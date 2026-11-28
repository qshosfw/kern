//! Portable RigWire v2 Data & Storage Channel (`CH_DATA = 0x03`) Service.
//!
//! Operates directly over `kernfs::KernFS` (`Fd` handles) and a minimal host hook
//! for write-authentication, staging buffer access, and calibration commit.

use super::cursor::WireCursor;
use super::ops_data::*;
use abi::Status;

pub const CALIB_PATH: &str = "/sys/calib.dat";

pub trait WireDataHost {
    fn is_authenticated(&self) -> bool;
    fn require_write_auth(&self) -> Result<(), Status>;
    fn staging_buf(&mut self) -> &mut [u8; 512];
    fn commit_calib<D: kernfs::BlockDevice>(
        &mut self,
        fs: &mut kernfs::KernFS<'_, D>,
    ) -> Result<u32, Status>;
    fn on_path_mutated(&mut self, path: &str);
}

#[inline(never)]
fn validate_regular_file<D: kernfs::BlockDevice>(
    fs: &mut kernfs::KernFS<'_, D>,
    path: &str,
    is_write: bool,
    allow_missing_or_calib: bool,
) -> Result<u32, Status> {
    let b = path.as_bytes();
    if b.len() > 32 || !utils::path::is_clean_path(b) {
        return Err(Status::InvalidArg);
    }
    let root = utils::path::root_bit(b);
    if root == 0
        || (is_write
            && root == utils::path::ROOT_SYS
            && (!allow_missing_or_calib || path != CALIB_PATH))
    {
        return Err(Status::SafetyBlocked);
    }
    match fs.stat_info(path) {
        Ok((_, true)) => Err(Status::InvalidArg),
        Ok((sz, false)) => Ok(sz as u32),
        Err(_) if allow_missing_or_calib => Ok(0),
        Err(_) => Err(Status::NotFound),
    }
}

#[inline(never)]
fn stream_file_crc32<D: kernfs::BlockDevice>(
    fs: &mut kernfs::KernFS<'_, D>,
    path: &str,
    size: usize,
    buf: &mut [u8; 512],
) -> u32 {
    let Ok(fd) = fs.open(path, false) else {
        return 0;
    };
    let mut crc = 0xFFFF_FFFFu32;
    let mut off = 0usize;
    let chunk = &mut buf[..256];
    while off < size {
        let to_read = (size - off).min(chunk.len());
        match fs.read_fd(fd, off, &mut chunk[..to_read]) {
            Ok(n) if n > 0 => {
                crc = crypto::crc::CrcEngine::crc32_update(crc, &chunk[..n]);
                off += n;
            }
            _ => break,
        }
    }
    !crc
}

#[inline(always)]
fn stat_or_root<D: kernfs::BlockDevice>(
    fs: &mut kernfs::KernFS<'_, D>,
    path: &str,
) -> Result<(usize, bool), kernfs::FsError> {
    let p = trim_trailing_slash(path);
    if p == "/" {
        Ok((0, true))
    } else {
        fs.stat_info(p)
    }
}

#[inline(always)]
fn require_active_transfer(t: &mut ActiveTransfer, tid: u16, now: u32) -> Result<(), Status> {
    if !t.active || t.transfer_id != tid {
        return Err(Status::BadSession);
    }
    t.last_activity_ms = now;
    Ok(())
}

/// Dispatch a `CH_DATA` (0x03) opcode directly against `KernFS`.
#[inline(never)]
pub fn dispatch_data_op<D: kernfs::BlockDevice, H: WireDataHost>(
    opcode: u8,
    c: &mut WireCursor<'_>,
    fs: &mut kernfs::KernFS<'_, D>,
    t: &mut ActiveTransfer,
    next_tid: &mut u16,
    now: u32,
    host: &mut H,
) -> Option<Status> {
    let res: Result<(), Status> = (|| match opcode {
        OP_TRANSFER_OPEN => {
            let mode = c.rd_u8()?;
            let total_size = c.rd_u32()?;
            let req_chunk = c.rd_u16()?;
            let path = c.rd_path()?;
            if mode > TRANSFER_MODE_WRITE {
                return Err(Status::InvalidArg);
            }
            let is_write = mode == TRANSFER_MODE_WRITE;
            if is_write {
                host.require_write_auth()?;
            }
            let existing = validate_regular_file(fs, path, is_write, is_write)?;
            let open_ino = if is_write {
                let req_sz = total_size as usize;
                if req_sz > MAX_WIRE_FILE_SIZE {
                    return Err(Status::BadLength);
                }
                let is_calib = path == CALIB_PATH;
                if is_calib {
                    if !host.is_authenticated() {
                        return Err(Status::PermissionDenied);
                    }
                    if req_sz != 512 {
                        return Err(Status::SafetyBlocked);
                    }
                }
                if req_sz > fs.stats().avail_bytes() + existing as usize {
                    return Err(Status::NoSpace);
                }
                if !is_calib && fs.write_file(path, &[]).is_err() {
                    return Err(Status::IoError);
                }
                0
            } else {
                fs.open(path, false).map_err(|_| Status::NotFound)?.ino
            };

            let tid = *next_tid;
            *next_tid = tid.wrapping_add(1).max(1);
            let chunk_size = req_chunk.clamp(16, c.max_out.saturating_sub(8).min(256) as u16);
            *t = ActiveTransfer::empty();
            t.active = true;
            t.mode = mode;
            t.transfer_id = tid;
            t.chunk_size = chunk_size;
            t.ino = open_ino;
            t.total_size = total_size;
            t.last_activity_ms = now;
            t.path.set(path);

            c.wr_u16(tid);
            c.wr_u16(chunk_size);
            c.wr_u32(existing);
            Ok(())
        }
        OP_TRANSFER_CHUNK => {
            let tid = c.rd_u16()?;
            let off = c.rd_u32()? as usize;
            let length = c.rd_u16()?;
            require_active_transfer(t, tid, now)?;
            if t.mode == TRANSFER_MODE_WRITE {
                let data = c.inp;
                let end = off.checked_add(data.len()).ok_or(Status::InvalidArg)?;
                if end > MAX_WIRE_FILE_SIZE {
                    return Err(Status::BadLength);
                }
                if t.path_str() == CALIB_PATH {
                    let staging = host.staging_buf();
                    if end > staging.len() {
                        return Err(Status::BadLength);
                    }
                    staging[off..end].copy_from_slice(data);
                } else {
                    fs.write_at(t.path_str(), off, data)
                        .map_err(|_| Status::IoError)?;
                }
                t.current_len = t.current_len.max(end as u32);
                c.wr_u16(tid);
                c.wr_u16(data.len() as u16);
                c.wr_u32(end as u32);
            } else {
                let len = (length as usize).min(c.max_out.saturating_sub(8).min(256));
                let n = fs
                    .read_ino_at(t.ino, off, &mut c.out[8..8 + len])
                    .unwrap_or(0);
                c.wr_u16(tid);
                c.wr_u32(off as u32);
                c.wr_u16(n as u16);
                c.pos = 8 + n;
            }
            Ok(())
        }
        OP_TRANSFER_CLOSE => {
            let tid = c.rd_u16()?;
            let expected_crc = c.rd_u32()?;
            require_active_transfer(t, tid, now)?;
            t.active = false;
            let mut actual_crc = 0u32;
            if t.mode == TRANSFER_MODE_WRITE {
                let total_len = t.current_len as usize;
                let path = t.path_str();
                if path == CALIB_PATH {
                    if total_len != 512 {
                        return Err(Status::SafetyBlocked);
                    }
                    actual_crc = host.commit_calib(fs)?;
                    if expected_crc != 0 && expected_crc != actual_crc {
                        return Err(Status::CorruptData);
                    }
                } else {
                    actual_crc = stream_file_crc32(fs, path, total_len, host.staging_buf());
                    if expected_crc != 0 && expected_crc != actual_crc {
                        let _ = fs.remove(path);
                        return Err(Status::CorruptData);
                    }
                }
                host.on_path_mutated(path);
            }
            c.wr_u16(tid);
            c.wr_u32(actual_crc);
            Ok(())
        }
        OP_TRANSFER_ABORT => {
            let tid = c.rd_u16()?;
            let _ = c.rd_u8()?;
            if t.active && t.transfer_id == tid {
                t.active = false;
                let path = t.path_str();
                if t.mode == TRANSFER_MODE_WRITE && path != CALIB_PATH {
                    let _ = fs.remove(path);
                    host.on_path_mutated(path);
                }
            }
            c.wr_u16(tid);
            Ok(())
        }
        OP_TRANSFER_STAT => {
            let path = c.rd_path()?;
            let Ok((sz, is_dir)) = stat_or_root(fs, path) else {
                c.zeroes(10);
                return Ok(());
            };
            let sz = if is_dir { 0 } else { sz };
            let crc = if sz > 0 {
                stream_file_crc32(fs, path, sz, host.staging_buf())
            } else {
                0
            };
            c.wr_u16(1 | (u16::from(is_dir) << 8));
            c.wr_u32(sz as u32);
            c.wr_u32(crc);
            Ok(())
        }
        OP_TRANSFER_LIST => {
            let start = c.rd_u16()?;
            let path = trim_trailing_slash(c.rd_path()?);
            if t.active && now.wrapping_sub(t.last_activity_ms) >= 5_000 {
                t.active = false;
            }
            if t.active && t.mode == TRANSFER_MODE_WRITE {
                return Err(Status::Busy);
            }
            match stat_or_root(fs, path) {
                Ok((_, true)) => {}
                Ok((_, false)) => return Err(Status::InvalidArg),
                Err(_) => return Err(Status::NotFound),
            }
            let mut idx = 0u16;
            let mut next_cursor = 0xFFFFu16;
            let mut written = 0u8;
            let max_pos = c.max_out;
            c.pos = 3;
            let _ = fs.list_dir(path, |name, size, _perm, is_dir| {
                if next_cursor == 0xFFFF {
                    if idx >= start {
                        let b = name.as_bytes();
                        let nlen = b.len().min(24);
                        if c.pos + 6 + nlen > max_pos {
                            next_cursor = idx;
                            return;
                        }
                        c.wr_u8(is_dir as u8);
                        c.wr_u32(size as u32);
                        c.wr_u8(nlen as u8);
                        c.put(&b[..nlen]);
                        written += 1;
                    }
                    idx += 1;
                }
            });
            c.out[0] = next_cursor as u8;
            c.out[1] = (next_cursor >> 8) as u8;
            c.out[2] = written;
            Ok(())
        }
        OP_TRANSFER_REMOVE => {
            host.require_write_auth()?;
            let path = c.rd_path()?;
            validate_regular_file(fs, path, true, false)?;
            fs.remove(path).map_err(|_| Status::NotFound)?;
            host.on_path_mutated(path);
            Ok(())
        }
        OP_TRANSFER_SECTOR_HASH => {
            let start_page = c.rd_u16()? as usize;
            let path = c.rd_path()?;
            let sz = validate_regular_file(fs, path, false, false)? as usize;
            let fd = fs.open(path, false).map_err(|_| Status::NotFound)?;
            let total_pages = (sz + 255) >> 8;
            let max_hashes = (c.max_out.saturating_sub(3) >> 2).min(64);
            let count = total_pages.saturating_sub(start_page).min(max_hashes);
            c.wr_u16(total_pages as u16);
            c.wr_u8(count as u8);
            let buf = &mut host.staging_buf()[..256];
            for page in start_page..start_page + count {
                let n = fs.read_fd(fd, page << 8, buf).unwrap_or(0);
                c.wr_u32(crypto::crc::CrcEngine::crc32(&buf[..n]));
            }
            Ok(())
        }
        _ => return Err(Status::Unsupported),
    })();
    if opcode == 0 || opcode > OP_TRANSFER_SECTOR_HASH {
        None
    } else {
        Some(res.err().unwrap_or(Status::Ok))
    }
}
