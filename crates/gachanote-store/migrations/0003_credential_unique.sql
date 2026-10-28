-- 0003：每个账号每种凭证只保存一条（鹰角 token 覆盖保存）。v0.1 从未写入 credential 表。
CREATE UNIQUE INDEX ux_credential_kind ON credential(account_id, kind);
