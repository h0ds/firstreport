//! Checkpoint of the active set (plan §5.3): custom binary, versioned
//! header, xxhash trailer, written atomically. On load, any mismatch
//! (magic, version, checksum, truncation) is an error and the caller
//! rebuilds from the store instead.

use std::io::{self, Read, Write};
use std::path::Path;

use xxhash_rust::xxh64::xxh64;

use super::active::{ActiveCluster, Member};

const MAGIC: &[u8; 4] = b"KGCK";
const VERSION: u32 = 1;

struct W(Vec<u8>);
impl W {
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn i64(&mut self, v: i64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn str(&mut self, s: &str) {
        self.u32(s.len() as u32);
        self.0.extend_from_slice(s.as_bytes());
    }
    fn f32s(&mut self, v: &[f32]) {
        for x in v {
            self.0.extend_from_slice(&x.to_le_bytes());
        }
    }
}

struct R<'a>(&'a [u8]);
impl R<'_> {
    fn take(&mut self, n: usize) -> io::Result<&[u8]> {
        if self.0.len() < n {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "truncated checkpoint",
            ));
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Ok(a)
    }
    fn u32(&mut self) -> io::Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> io::Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn i64(&mut self) -> io::Result<i64> {
        Ok(i64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn str(&mut self) -> io::Result<String> {
        let n = self.u32()? as usize;
        String::from_utf8(self.take(n)?.to_vec())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }
    fn f32s(&mut self, n: usize) -> io::Result<Vec<f32>> {
        let b = self.take(n * 4)?;
        Ok(b.chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect())
    }
}

pub fn encode(active: &[ActiveCluster], dim: usize) -> Vec<u8> {
    let mut w = W(Vec::with_capacity(1024));
    w.0.extend_from_slice(MAGIC);
    w.u32(VERSION);
    w.u32(dim as u32);
    w.u32(active.len() as u32);
    for c in active {
        w.u64(c.id);
        w.i64(c.created_at);
        w.i64(c.last_active_at);
        w.u32(c.members.len() as u32);
        for (m, v) in c.members.iter().zip(&c.vectors) {
            w.u64(m.item_id);
            w.str(&m.outlet_id);
            w.i64(m.published_at);
            w.i64(m.fetched_at);
            w.i64(m.joined_at);
            w.u64(m.text_hash);
            w.str(&m.title);
            debug_assert_eq!(v.len(), dim);
            w.f32s(v);
        }
    }
    let sum = xxh64(&w.0, 0);
    w.u64(sum);
    w.0
}

pub fn decode(bytes: &[u8]) -> io::Result<(Vec<ActiveCluster>, usize)> {
    let bad = |m: &str| io::Error::new(io::ErrorKind::InvalidData, m.to_string());
    if bytes.len() < 8 {
        return Err(bad("too short"));
    }
    let (body, trailer) = bytes.split_at(bytes.len() - 8);
    let want = u64::from_le_bytes(trailer.try_into().unwrap());
    if xxh64(body, 0) != want {
        return Err(bad("checksum mismatch"));
    }
    let mut r = R(body);
    if r.take(4)? != MAGIC {
        return Err(bad("bad magic"));
    }
    let version = r.u32()?;
    if version != VERSION {
        return Err(bad(&format!("unsupported checkpoint version {version}")));
    }
    let dim = r.u32()? as usize;
    let count = r.u32()? as usize;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let id = r.u64()?;
        let created_at = r.i64()?;
        let last_active_at = r.i64()?;
        let n = r.u32()? as usize;
        let mut members = Vec::with_capacity(n);
        let mut vectors = Vec::with_capacity(n);
        for _ in 0..n {
            members.push(Member {
                item_id: r.u64()?,
                outlet_id: r.str()?,
                published_at: r.i64()?,
                fetched_at: r.i64()?,
                joined_at: r.i64()?,
                text_hash: r.u64()?,
                title: r.str()?,
            });
            vectors.push(r.f32s(dim)?);
        }
        out.push(ActiveCluster::from_parts(
            id,
            created_at,
            last_active_at,
            members,
            vectors,
        ));
    }
    if !r.0.is_empty() {
        return Err(bad("trailing bytes"));
    }
    Ok((out, dim))
}

/// Write atomically: temp file in the same directory, fsync, rename.
pub fn write(path: &Path, active: &[ActiveCluster], dim: usize) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let tmp = path.with_extension("ckpt.tmp");
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(&encode(active, dim))?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

pub fn read(path: &Path) -> io::Result<(Vec<ActiveCluster>, usize)> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?.read_to_end(&mut bytes)?;
    decode(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<ActiveCluster> {
        let m = |id: u64, o: &str| Member {
            item_id: id,
            outlet_id: o.into(),
            published_at: 10 * id as i64,
            fetched_at: 10 * id as i64 + 1,
            joined_at: 10 * id as i64 + 2,
            text_hash: id * 7,
            title: format!("title {id} 鏡"),
        };
        let mut a = ActiveCluster::new(m(1, "bbc"), vec![1.0, 0.0, 0.0], 12);
        a.add(m(2, "ap"), vec![0.0, 1.0, 0.0], 22);
        let b = ActiveCluster::new(m(3, "rt"), vec![0.0, 0.0, 1.0], 32);
        vec![a, b]
    }

    #[test]
    fn roundtrip() {
        let active = sample();
        let bytes = encode(&active, 3);
        let (back, dim) = decode(&bytes).unwrap();
        assert_eq!(dim, 3);
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].id, 1);
        assert_eq!(back[0].members, active[0].members);
        assert_eq!(back[0].vectors, active[0].vectors);
        assert_eq!(back[0].centroid, active[0].centroid);
        assert_eq!(back[0].last_active_at, 22);
        assert_eq!(back[0].outlet_count(), 2);
        assert_eq!(back[1].members[0].title, "title 3 鏡");
    }

    #[test]
    fn corruption_is_detected() {
        let mut bytes = encode(&sample(), 3);
        assert!(decode(&bytes[..bytes.len() - 1]).is_err(), "truncated");
        bytes[20] ^= 0xFF;
        assert!(decode(&bytes).is_err(), "flipped byte");
        assert!(decode(b"KGCK").is_err());
    }

    #[test]
    fn file_roundtrip_is_atomic() {
        let dir = std::env::temp_dir().join(format!("kg-ckpt-{}", std::process::id()));
        let path = dir.join("clusterer.ckpt");
        write(&path, &sample(), 3).unwrap();
        assert!(!path.with_extension("ckpt.tmp").exists());
        assert_eq!(read(&path).unwrap().0.len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
