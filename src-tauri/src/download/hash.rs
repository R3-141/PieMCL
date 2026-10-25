// 流式哈希校验
//
// 约束：哈希必须流式计算，禁止把整个文件读入内存——
// client.jar 可达 25MB+，内存固定使用 1MB 缓冲逐块喂给 Digest。

use std::path::Path;

use sha1::{Digest, Sha1};
use sha2::{Sha256, Sha512};
use tokio::io::AsyncReadExt;

use super::model::HashAlgo;
use crate::core::error::AppResult;

/// 缓冲区大小：1MB
const BUF_SIZE: usize = 1024 * 1024;

/// 计算文件哈希（小写 hex）
pub async fn hash_file(path: &Path, algo: HashAlgo) -> AppResult<String> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut buf = vec![0u8; BUF_SIZE];
    let mut digest = new_digest(algo);
    loop {
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        digest.update(&buf[..n]);
    }
    Ok(hex::encode(digest.finalize()))
}

/// 校验文件哈希是否匹配（期望值为小写 hex）
pub async fn verify_file(path: &Path, algo: HashAlgo, expected_hex: &str) -> AppResult<bool> {
    Ok(hash_file(path, algo).await?.eq_ignore_ascii_case(expected_hex))
}

fn new_digest(algo: HashAlgo) -> Box<dyn DigestDyn> {
    match algo {
        HashAlgo::Sha1 => Box::new(Sha1::new()),
        HashAlgo::Sha256 => Box::new(Sha256::new()),
        HashAlgo::Sha512 => Box::new(Sha512::new()),
    }
}

/// 统一三个哈希器的更新/完成接口（Send：在异步任务中跨 await 持有）
trait DigestDyn: Send {
    fn update(&mut self, data: &[u8]);
    fn finalize(self: Box<Self>) -> Vec<u8>;
}

impl DigestDyn for Sha1 {
    fn update(&mut self, data: &[u8]) {
        Digest::update(self, data);
    }
    fn finalize(self: Box<Self>) -> Vec<u8> {
        Digest::finalize(*self).to_vec()
    }
}

impl DigestDyn for Sha256 {
    fn update(&mut self, data: &[u8]) {
        Digest::update(self, data);
    }
    fn finalize(self: Box<Self>) -> Vec<u8> {
        Digest::finalize(*self).to_vec()
    }
}

impl DigestDyn for Sha512 {
    fn update(&mut self, data: &[u8]) {
        Digest::update(self, data);
    }
    fn finalize(self: Box<Self>) -> Vec<u8> {
        Digest::finalize(*self).to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_temp(data: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("piemcl-hash-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("data.bin");
        std::fs::write(&p, data).unwrap();
        p
    }

    #[tokio::test]
    async fn known_vectors_abc() {
        // 标准测试向量 "abc"
        let p = write_temp(b"abc");
        assert_eq!(
            hash_file(&p, HashAlgo::Sha1).await.unwrap(),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(
            hash_file(&p, HashAlgo::Sha256).await.unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            hash_file(&p, HashAlgo::Sha512).await.unwrap(),
            "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
        );
        let dir = p.parent().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn larger_than_buffer_streaming() {
        // 3MB 数据跨多个 1MB 缓冲块，验证流式计算正确
        let data: Vec<u8> = (0..3 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
        let p = write_temp(&data);

        // 与一次性计算对照
        let mut one_shot = Sha256::new();
        Digest::update(&mut one_shot, &data);
        let expected = hex::encode(Digest::finalize(one_shot));

        assert_eq!(hash_file(&p, HashAlgo::Sha256).await.unwrap(), expected);
        assert!(verify_file(&p, HashAlgo::Sha256, &expected).await.unwrap());
        assert!(!verify_file(&p, HashAlgo::Sha256, "deadbeef").await.unwrap());

        let dir = p.parent().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }
}
