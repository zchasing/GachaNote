//! 凭证加密（T-50，FR-FET-09、NFR-07）：Windows 上用 DPAPI 绑定当前 Windows 用户，
//! 换一个用户或换一台电脑都解不开；数据库里只存密文。
//!
//! 其他平台（只有开发与 CI）没有对应的系统能力，不保存 token，界面上隐藏「记住 token」。

use gachanote_core::repo::SecretStore;

/// 附加熵：同一用户下的其他程序不知道它，也就无法直接调用 DPAPI 解密。
#[cfg_attr(not(windows), allow(dead_code))]
const ENTROPY: &[u8] = b"GachaNote credential v1";

/// 当前平台的凭证加密；不支持时为空。
pub fn platform_store() -> Option<Box<dyn SecretStore>> {
    #[cfg(windows)]
    {
        Some(Box::new(dpapi::Dpapi))
    }
    #[cfg(not(windows))]
    {
        None
    }
}

#[cfg(windows)]
mod dpapi {
    //! `CryptProtectData` / `CryptUnprotectData` 的最小封装。
    #![allow(unsafe_code)]

    use std::ptr;

    use gachanote_core::repo::SecretStore;
    use gachanote_core::{AppError, Result};
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    use super::ENTROPY;

    /// DPAPI（CurrentUser 范围）。
    pub struct Dpapi;

    fn blob(data: &[u8]) -> Result<CRYPT_INTEGER_BLOB> {
        Ok(CRYPT_INTEGER_BLOB {
            cbData: u32::try_from(data.len())
                .map_err(|_| AppError::Invalid("credential too large".into()))?,
            pbData: data.as_ptr().cast_mut(),
        })
    }

    /// 把 DPAPI 分配的输出复制出来并释放。
    ///
    /// # Safety
    /// `out` 必须是 DPAPI 调用成功后返回的缓冲区。
    unsafe fn take(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
        if out.pbData.is_null() {
            return Vec::new();
        }
        // SAFETY: 调用成功时 pbData 指向 cbData 字节、由 LocalAlloc 分配的缓冲区
        let v = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize) }.to_vec();
        // SAFETY: 同上，缓冲区只释放这一次
        unsafe { LocalFree(out.pbData.cast()) };
        v
    }

    impl SecretStore for Dpapi {
        fn protect(&self, plain: &[u8]) -> Result<Vec<u8>> {
            let input = blob(plain)?;
            let entropy = blob(ENTROPY)?;
            let mut out = CRYPT_INTEGER_BLOB {
                cbData: 0,
                pbData: ptr::null_mut(),
            };
            // SAFETY: 输入缓冲区在调用期间有效；DPAPI 只读取它们，输出由 take 释放
            let ok = unsafe {
                CryptProtectData(
                    &input,
                    ptr::null(),
                    &entropy,
                    ptr::null(),
                    ptr::null(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut out,
                )
            };
            if ok == 0 {
                return Err(AppError::Internal(format!(
                    "CryptProtectData failed: {}",
                    std::io::Error::last_os_error()
                )));
            }
            // SAFETY: 调用成功
            Ok(unsafe { take(out) })
        }

        fn unprotect(&self, data: &[u8]) -> Result<Vec<u8>> {
            let input = blob(data)?;
            let entropy = blob(ENTROPY)?;
            let mut out = CRYPT_INTEGER_BLOB {
                cbData: 0,
                pbData: ptr::null_mut(),
            };
            // SAFETY: 同 protect
            let ok = unsafe {
                CryptUnprotectData(
                    &input,
                    ptr::null_mut(),
                    &entropy,
                    ptr::null(),
                    ptr::null(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut out,
                )
            };
            if ok == 0 {
                // 密文损坏、换了 Windows 用户或换了电脑
                return Err(AppError::AuthExpired);
            }
            // SAFETY: 调用成功
            Ok(unsafe { take(out) })
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn round_trip_and_tamper() {
            let d = Dpapi;
            let blob = d.protect(b"hg-token").unwrap();
            assert!(
                !blob.windows(8).any(|w| w == b"hg-token"),
                "密文里不能有明文"
            );
            assert_eq!(d.unprotect(&blob).unwrap(), b"hg-token");
            let mut bad = blob.clone();
            let last = bad.len() - 1;
            bad[last] ^= 0xFF;
            assert_eq!(d.unprotect(&bad).unwrap_err(), AppError::AuthExpired);
            assert_eq!(d.unprotect(&d.protect(b"").unwrap()).unwrap(), b"");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_only_on_windows() {
        assert_eq!(platform_store().is_some(), cfg!(windows));
    }
}
