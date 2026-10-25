// 实例 servers.dat 的 NBT 读写：把启动器收藏服务器合并进实例服务器列表
//
// servers.dat 为未压缩 NBT，结构（1.7.6+）：
//   TAG_Compound("") { TAG_List("servers") [ TAG_Compound {
//       TAG_String("ip") = "host:port", TAG_String("name") = "...",
//       [TAG_Byte("bypassesPrompt")], [TAG_ByteArray("icon")] } ... ] }
// MC 运行时会读写该文件，导入前由调用方提示用户先关闭对应实例。

use std::collections::HashMap;
use std::path::Path;

use fastnbt::Value;

use super::store::ServerEntry;

/// 从 NBT Compound 取字符串字段
fn get_str<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    match v {
        Value::Compound(m) => match m.get(key) {
            Some(Value::String(s)) => Some(s.as_str()),
            _ => None,
        },
        _ => None,
    }
}

/// 把收藏服务器合并进实例工作目录的 servers.dat（按 ip 忽略大小写去重，
/// 已存在的跳过；实例没有 servers.dat 时新建）。返回 (导入数, 跳过数)。
pub fn merge_to_instance_dat(
    game_dir: &Path,
    entries: &[ServerEntry],
) -> Result<(usize, usize), String> {
    if entries.is_empty() {
        return Err("收藏列表为空".into());
    }
    let dat_path = game_dir.join("servers.dat");

    // 读取现有服务器（缺文件 = 空列表；解析失败报错保护用户数据）
    let mut root: HashMap<String, Value> = match std::fs::read(&dat_path) {
        Ok(data) => match fastnbt::from_bytes(&data)
            .map_err(|e| format!("解析现有 servers.dat 失败：{e}"))?
        {
            Value::Compound(m) => m,
            _ => return Err("servers.dat 结构异常（根不是 Compound）".into()),
        },
        Err(_) if !dat_path.exists() => HashMap::new(),
        Err(e) => return Err(format!("读取 servers.dat 失败：{e}")),
    };

    // 取出现有 servers 列表
    let mut servers: Vec<Value> = match root.remove("servers") {
        Some(Value::List(l)) => l,
        Some(_) => return Err("servers.dat 的 servers 字段类型异常".into()),
        None => Vec::new(),
    };

    // 按 ip 去重合并（收藏列表顺序在前）
    let mut imported = 0usize;
    let mut skipped = 0usize;
    let mut new_items: Vec<Value> = Vec::new();
    for e in entries {
        let addr_lower = e.address.to_ascii_lowercase();
        let dup = |v: &Value| get_str(v, "ip").map(|s| s.to_ascii_lowercase()) == Some(addr_lower.clone());
        if servers.iter().any(&dup) || new_items.iter().any(&dup) {
            skipped += 1;
            continue;
        }
        let mut compound = HashMap::new();
        compound.insert("ip".to_string(), Value::String(e.address.clone()));
        compound.insert("name".to_string(), Value::String(e.name.clone()));
        compound.insert("bypassesPrompt".to_string(), Value::Byte(0));
        new_items.push(Value::Compound(compound));
        imported += 1;
    }
    if imported == 0 {
        return Ok((0, skipped));
    }

    // 新服务器追加到列表尾部（保持已有顺序）
    servers.extend(new_items);
    root.insert("servers".to_string(), Value::List(servers));

    // 写回（未压缩 NBT；原子写：先写临时文件再改名）
    let bytes = fastnbt::to_bytes(&Value::Compound(root))
        .map_err(|e| format!("序列化 servers.dat 失败：{e}"))?;
    let tmp = dat_path.with_extension("dat.tmp");
    std::fs::write(&tmp, bytes).map_err(|e| format!("写入 servers.dat 失败：{e}"))?;
    // Windows 上 rename 前需先删旧目标
    if dat_path.exists() {
        std::fs::remove_file(&dat_path).map_err(|e| format!("替换 servers.dat 失败：{e}"))?;
    }
    std::fs::rename(&tmp, &dat_path).map_err(|e| format!("落位 servers.dat 失败：{e}"))?;
    Ok((imported, skipped))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, addr: &str) -> ServerEntry {
        ServerEntry {
            id: uuid::Uuid::new_v4().simple().to_string(),
            name: name.into(),
            address: addr.into(),
        }
    }

    #[test]
    fn merge_creates_and_dedups() {
        let base = std::env::temp_dir().join(format!("piemcl-nbt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let game_dir = base.join("versions").join("t1");
        std::fs::create_dir_all(&game_dir).unwrap();

        // 首次导入：2 条全导入
        let (imp, skip) =
            merge_to_instance_dat(&game_dir, &[entry("A", "a.com:25565"), entry("B", "b.com")])
                .unwrap();
        assert_eq!((imp, skip), (2, 0));

        // 再次导入：同地址去重 + 新增一条
        let (imp, skip) = merge_to_instance_dat(
            &game_dir,
            &[
                entry("A2", "A.COM:25565"), // 大小写不敏感去重
                entry("C", "c.com:1234"),
            ],
        )
        .unwrap();
        assert_eq!((imp, skip), (1, 1));

        // 验证文件结构：手动解析
        let data = std::fs::read(game_dir.join("servers.dat")).unwrap();
        let v: Value = fastnbt::from_bytes(&data).unwrap();
        let servers = match v {
            Value::Compound(m) => match m.get("servers") {
                Some(Value::List(l)) => l.clone(),
                _ => panic!("servers 字段缺失"),
            },
            _ => panic!("根不是 Compound"),
        };
        assert_eq!(servers.len(), 3);
        let ips: Vec<String> = servers
            .iter()
            .filter_map(|v| get_str(v, "ip").map(String::from))
            .collect();
        assert_eq!(ips, vec!["a.com:25565", "b.com", "c.com:1234"]);
        let _ = std::fs::remove_dir_all(&base);
    }
}
