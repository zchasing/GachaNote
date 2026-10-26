-- 记录页按时间倒序翻页（不限卡池时）：没有这个索引时每次查询都要把账号的全部记录排序一遍。
CREATE INDEX ix_record_account_time ON gacha_record(account_id, pulled_at, seq_in_sec, id);
