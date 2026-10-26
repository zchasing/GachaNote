//! 长任务调度：取消、进度与「同一账号同时只允许一个写入任务」（架构文档 5.3）。
//!
//! 任务在独立线程上运行。取消只是置位 [`CancelToken`]，由任务在各阶段之间和写入事务中
//! 主动检查；被取消或失败时事务回滚，数据库保持原样。

use std::collections::{HashMap, HashSet};
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, Result};
use crate::import::ImportReport;

/// 任务 ID，进程内递增。
pub type TaskId = u64;

/// 取消标记，可在线程间共享。
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    /// 新的未取消标记。
    pub fn new() -> Self {
        Self::default()
    }

    /// 请求取消。
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// 是否已请求取消。
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }

    /// 已取消时返回 [`AppError::Cancelled`]。
    pub fn check(&self) -> Result<()> {
        if self.is_cancelled() {
            Err(AppError::Cancelled)
        } else {
            Ok(())
        }
    }
}

/// 任务阶段（`task://progress` 的 `stage`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    /// 查找链接或凭据。
    Discover,
    /// 拉取。
    Fetch,
    /// 解析与规范化。
    Parse,
    /// 备份。
    Backup,
    /// 写入数据库。
    Write,
}

/// 一条进度。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgressEvent {
    /// 阶段。
    pub stage: Stage,
    /// 当前进度。
    pub current: u64,
    /// 总量（未知时为空）。
    pub total: Option<u64>,
    /// 说明（如当前卡池），不含敏感信息。
    pub message: String,
}

/// 进度接收方。
pub trait Progress: Send + Sync {
    /// 报告一条进度。
    fn report(&self, event: ProgressEvent);
}

/// 不关心进度时使用。
#[derive(Debug, Clone, Copy, Default)]
pub struct NoProgress;

impl Progress for NoProgress {
    fn report(&self, _event: ProgressEvent) {}
}

/// 任务事件接收方：外壳把它们转成 `task://progress`、`task://done`、`task://failed`。
pub trait TaskEvents: Send + Sync {
    /// 进度。
    fn progress(&self, id: TaskId, event: ProgressEvent);
    /// 成功完成。
    fn done(&self, id: TaskId, report: &ImportReport);
    /// 失败或被取消（数据已回滚）。
    fn failed(&self, id: TaskId, error: &AppError);
}

#[derive(Default)]
struct Inner {
    running: HashMap<TaskId, CancelToken>,
    claimed: HashSet<String>,
}

fn lock(inner: &Mutex<Inner>) -> std::sync::MutexGuard<'_, Inner> {
    inner.lock().unwrap_or_else(PoisonError::into_inner)
}

/// 长任务调度器。
#[derive(Default)]
pub struct TaskManager {
    next_id: AtomicU64,
    inner: Arc<Mutex<Inner>>,
}

/// 任务运行时可用的上下文。
pub struct TaskContext {
    id: TaskId,
    cancel: CancelToken,
    events: Arc<dyn TaskEvents>,
    inner: Arc<Mutex<Inner>>,
}

/// 已占用的账号写入权，丢弃时释放。
pub struct AccountClaim {
    keys: Vec<String>,
    inner: Arc<Mutex<Inner>>,
}

impl Drop for AccountClaim {
    fn drop(&mut self) {
        let mut inner = lock(&self.inner);
        for k in &self.keys {
            inner.claimed.remove(k);
        }
    }
}

impl TaskContext {
    /// 任务 ID。
    pub fn id(&self) -> TaskId {
        self.id
    }

    /// 取消标记。
    pub fn cancel_token(&self) -> &CancelToken {
        &self.cancel
    }

    /// 占用一组账号（键通常为 `game:uid`）的写入权；任一账号已被其他任务占用时返回
    /// [`AppError::Busy`]，且不占用任何账号。
    pub fn claim<I, S>(&self, keys: I) -> Result<AccountClaim>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let keys: Vec<String> = keys
            .into_iter()
            .map(Into::into)
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        let mut inner = lock(&self.inner);
        if let Some(busy) = keys.iter().find(|k| inner.claimed.contains(*k)) {
            return Err(AppError::Busy(busy.clone()));
        }
        inner.claimed.extend(keys.iter().cloned());
        Ok(AccountClaim {
            keys,
            inner: Arc::clone(&self.inner),
        })
    }
}

impl Progress for TaskContext {
    fn report(&self, event: ProgressEvent) {
        self.events.progress(self.id, event);
    }
}

/// 已启动的任务。
pub struct TaskHandle {
    /// 任务 ID。
    pub id: TaskId,
    join: JoinHandle<Result<ImportReport>>,
}

impl TaskHandle {
    /// 等待任务结束并取得结果（测试与命令行使用；界面走事件）。
    pub fn wait(self) -> Result<ImportReport> {
        self.join
            .join()
            .unwrap_or_else(|_| Err(AppError::Internal("task thread panicked".into())))
    }
}

impl TaskManager {
    /// 新的调度器。
    pub fn new() -> Self {
        Self::default()
    }

    /// 在新线程上启动任务。结束时（成功、失败或取消）发出对应事件。
    pub fn spawn<F>(&self, events: Arc<dyn TaskEvents>, job: F) -> TaskHandle
    where
        F: FnOnce(&TaskContext) -> Result<ImportReport> + Send + 'static,
    {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst) + 1;
        let cancel = CancelToken::new();
        lock(&self.inner).running.insert(id, cancel.clone());
        let ctx = TaskContext {
            id,
            cancel,
            events,
            inner: Arc::clone(&self.inner),
        };
        let join = std::thread::spawn(move || {
            let result = std::panic::catch_unwind(AssertUnwindSafe(|| job(&ctx)))
                .unwrap_or_else(|_| Err(AppError::Internal("task panicked".into())));
            lock(&ctx.inner).running.remove(&ctx.id);
            match &result {
                Ok(report) => ctx.events.done(ctx.id, report),
                Err(e) => ctx.events.failed(ctx.id, e),
            }
            result
        });
        TaskHandle { id, join }
    }

    /// 请求取消任务。任务不存在或已结束时返回 `false`。
    pub fn cancel(&self, id: TaskId) -> bool {
        match lock(&self.inner).running.get(&id) {
            Some(token) => {
                token.cancel();
                true
            }
            None => false,
        }
    }

    /// 正在运行的任务数。
    pub fn running(&self) -> usize {
        lock(&self.inner).running.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[derive(Default)]
    struct Recorder(Mutex<Vec<String>>);

    impl TaskEvents for Recorder {
        fn progress(&self, id: TaskId, e: ProgressEvent) {
            self.0
                .lock()
                .unwrap()
                .push(format!("{id}:progress:{}", e.current));
        }
        fn done(&self, id: TaskId, r: &ImportReport) {
            self.0
                .lock()
                .unwrap()
                .push(format!("{id}:done:{}", r.added()));
        }
        fn failed(&self, id: TaskId, e: &AppError) {
            self.0
                .lock()
                .unwrap()
                .push(format!("{id}:failed:{}", e.code()));
        }
    }

    #[test]
    fn cancel_token() {
        let t = CancelToken::new();
        assert!(t.check().is_ok());
        t.clone().cancel();
        assert!(t.is_cancelled());
        assert_eq!(t.check(), Err(AppError::Cancelled));
    }

    #[test]
    fn spawn_reports_progress_and_done() {
        let tm = TaskManager::new();
        let rec = Arc::new(Recorder::default());
        let h = tm.spawn(rec.clone(), |ctx| {
            ctx.report(ProgressEvent {
                stage: Stage::Parse,
                current: 7,
                total: None,
                message: String::new(),
            });
            Ok(ImportReport::default())
        });
        let id = h.id;
        h.wait().unwrap();
        assert_eq!(
            *rec.0.lock().unwrap(),
            vec![format!("{id}:progress:7"), format!("{id}:done:0")]
        );
        assert_eq!(tm.running(), 0);
        assert!(!tm.cancel(id));
    }

    #[test]
    fn cancel_running_task() {
        let tm = TaskManager::new();
        let rec = Arc::new(Recorder::default());
        let (started_tx, started_rx) = mpsc::channel();
        let (go_tx, go_rx) = mpsc::channel::<()>();
        let h = tm.spawn(rec.clone(), move |ctx| {
            started_tx.send(()).unwrap();
            go_rx.recv().unwrap();
            ctx.cancel_token().check()?;
            Ok(ImportReport::default())
        });
        started_rx.recv().unwrap();
        assert_eq!(tm.running(), 1);
        assert!(tm.cancel(h.id));
        go_tx.send(()).unwrap();
        let id = h.id;
        assert_eq!(h.wait(), Err(AppError::Cancelled));
        assert_eq!(
            *rec.0.lock().unwrap(),
            vec![format!("{id}:failed:cancelled")]
        );
    }

    #[test]
    fn one_writer_per_account() {
        let tm = TaskManager::new();
        let rec = Arc::new(Recorder::default());
        let (claimed_tx, claimed_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let first = tm.spawn(rec.clone(), move |ctx| {
            let _claim = ctx.claim(["genshin:1", "genshin:1"])?;
            claimed_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            Ok(ImportReport::default())
        });
        claimed_rx.recv().unwrap();
        let second = tm.spawn(rec.clone(), |ctx| {
            let _claim = ctx.claim(["genshin:2", "genshin:1"])?;
            Ok(ImportReport::default())
        });
        assert_eq!(second.wait(), Err(AppError::Busy("genshin:1".into())));
        release_tx.send(()).unwrap();
        first.wait().unwrap();
        // 释放后（且失败的任务没有残留占用）可以再次占用
        let third = tm.spawn(rec, |ctx| {
            let _a = ctx.claim(["genshin:1"])?;
            let _b = ctx.claim(["genshin:2"])?;
            Ok(ImportReport::default())
        });
        third.wait().unwrap();
    }

    #[test]
    fn panicking_task_is_reported_as_internal() {
        let tm = TaskManager::new();
        let rec = Arc::new(Recorder::default());
        let h = tm.spawn(rec.clone(), |_| panic!("boom"));
        let id = h.id;
        assert_eq!(h.wait().unwrap_err().code(), "internal");
        assert_eq!(tm.running(), 0);
        assert_eq!(
            *rec.0.lock().unwrap(),
            vec![format!("{id}:failed:internal")]
        );
        NoProgress.report(ProgressEvent {
            stage: Stage::Write,
            current: 0,
            total: Some(1),
            message: String::new(),
        });
    }
}
