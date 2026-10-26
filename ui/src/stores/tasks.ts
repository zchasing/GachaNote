import { defineStore } from 'pinia'
import { reactive } from 'vue'
import type { ErrorDto, ImportReportDto, StageDto } from '@/bindings'
import { events } from '@/api'

export interface TaskState {
  status: 'running' | 'done' | 'failed'
  stage: StageDto | null
  current: number
  total: number | null
  message: string
  report: ImportReportDto | null
  error: ErrorDto | null
}

function blank(): TaskState {
  return {
    status: 'running',
    stage: null,
    current: 0,
    total: null,
    message: '',
    report: null,
    error: null,
  }
}

/**
 * 导入与拉取任务的状态，按任务 ID 记录。启动时就订阅 `task://*`，
 * 这样即使任务在命令返回 ID 之前就结束了，结果也不会丢。
 */
export const useTasksStore = defineStore('tasks', () => {
  const tasks = reactive(new Map<number, TaskState>())
  let listening: Promise<unknown> | null = null

  function entry(id: number): TaskState {
    let t = tasks.get(id)
    if (!t) {
      tasks.set(id, blank())
      t = tasks.get(id)!
    }
    return t
  }

  function listen() {
    listening ??= Promise.all([
      events.taskProgress.listen(({ payload }) => {
        const t = entry(payload.taskId)
        t.stage = payload.stage
        t.current = payload.current
        t.total = payload.total
        t.message = payload.message
      }),
      events.taskDone.listen(({ payload }) => {
        const t = entry(payload.taskId)
        t.status = 'done'
        t.report = payload.report
      }),
      events.taskFailed.listen(({ payload }) => {
        const t = entry(payload.taskId)
        t.status = 'failed'
        t.error = payload.error
      }),
    ])
    return listening
  }

  return { tasks, entry, listen }
})
