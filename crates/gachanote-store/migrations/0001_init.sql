-- 0001：初始表结构（架构文档第 4 节）。主键为 UUID v7 文本，时间为 UTC 毫秒。

CREATE TABLE account (
  id            TEXT PRIMARY KEY,
  game          TEXT NOT NULL,
  uid           TEXT NOT NULL,
  region        TEXT,
  nickname      TEXT,
  tz_offset_min INTEGER NOT NULL DEFAULT 480,
  created_at    INTEGER NOT NULL,
  UNIQUE (game, uid)
);

CREATE TABLE import_batch (
  id          TEXT PRIMARY KEY,
  account_id  TEXT REFERENCES account(id) ON DELETE CASCADE,
  source      TEXT NOT NULL,
  format      TEXT,
  file_name   TEXT,
  added       INTEGER NOT NULL,
  skipped     INTEGER NOT NULL,
  started_at  INTEGER NOT NULL,
  finished_at INTEGER
);
CREATE INDEX ix_batch_account ON import_batch(account_id);

CREATE TABLE gacha_record (
  id          TEXT PRIMARY KEY,
  account_id  TEXT NOT NULL REFERENCES account(id) ON DELETE CASCADE,
  pool_type   TEXT NOT NULL,
  banner_id   TEXT,
  ext_id      TEXT,
  item_id     TEXT,
  item_name   TEXT NOT NULL,
  item_type   TEXT,
  rarity      INTEGER NOT NULL,
  pulled_at   INTEGER NOT NULL,
  seq_in_sec  INTEGER NOT NULL DEFAULT 0,
  is_pull     INTEGER NOT NULL DEFAULT 1,
  batch_id    TEXT REFERENCES import_batch(id) ON DELETE SET NULL,
  raw_json    TEXT NOT NULL
);
-- 双去重索引：有官方 ID 时按 ID，没有时按自然键（FR-IMP-05）
CREATE UNIQUE INDEX ux_record_ext ON gacha_record(account_id, ext_id) WHERE ext_id IS NOT NULL;
CREATE UNIQUE INDEX ux_record_nat ON gacha_record(account_id, pool_type, pulled_at, item_name, seq_in_sec) WHERE ext_id IS NULL;
CREATE INDEX ix_record_timeline ON gacha_record(account_id, pool_type, pulled_at, seq_in_sec);
CREATE INDEX ix_record_rarity ON gacha_record(account_id, rarity);
CREATE INDEX ix_record_batch ON gacha_record(batch_id);

CREATE TABLE credential (
  id         TEXT PRIMARY KEY,
  account_id TEXT NOT NULL REFERENCES account(id) ON DELETE CASCADE,
  kind       TEXT NOT NULL,
  blob       BLOB NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE setting (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
