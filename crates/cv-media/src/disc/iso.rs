//! A read-only ISO 9660 and UDF reader, so a disc image can be analysed without mounting it.
//!
//! * **UDF** (1.02 to 2.60) is tried first: it is what Blu-ray images use, with the metadata
//!   partition of UDF 2.50, and it is the primary file system of most DVD images too.
//! * **ISO 9660** is the fallback: the bridge on older DVD images and plain data discs. Names
//!   are read from the base volume descriptor (no Joliet or Rock Ridge), which is enough for
//!   the 8.3 names disc structures use.
//!
//! Only 2048-byte sectors are supported. Sparing tables (UDF defect management) and virtual
//! partitions (UDF on rewritable media) are not: an image that needs them is reported as
//! unsupported, and the caller falls back to listing the largest stream file.

use std::io::{self, Read, Seek, SeekFrom};
use std::sync::Mutex;

use super::files::{DiscEntry, DiscFs};

const SECTOR: u64 = 2048;
/// The most a directory may hold; a bigger one is a corrupt image, not a disc.
const MAX_DIRECTORY: u64 = 16 * 1024 * 1024;
/// How deep path lookups go; real discs are three levels deep.
const MAX_DEPTH: usize = 16;

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn not_found(path: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::NotFound,
        format!("{path} is not on the disc"),
    )
}

fn le16(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn le32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn le64(bytes: &[u8], at: usize) -> u64 {
    let mut raw = [0_u8; 8];
    raw.copy_from_slice(&bytes[at..at + 8]);
    u64::from_le_bytes(raw)
}

/// A run of bytes in the image; `start` is `None` for a hole that reads as zeros.
#[derive(Clone, Debug)]
struct Extent {
    start: Option<u64>,
    len: u64,
}

#[derive(Clone, Debug)]
enum Data {
    Extents(Vec<Extent>),
    /// Data stored inside the descriptor (small UDF files).
    Inline(Vec<u8>),
}

/// A file or directory: its size and where its bytes are.
#[derive(Clone, Debug)]
struct Node {
    is_dir: bool,
    size: u64,
    data: Data,
}

struct Source<R> {
    inner: Mutex<R>,
    len: u64,
}

impl<R: Read + Seek> Source<R> {
    /// Reads as much of `buf` as the image holds at `offset`.
    fn read_some(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        if offset >= self.len {
            return Ok(0);
        }
        let want = usize::try_from((self.len - offset).min(buf.len() as u64)).unwrap_or(buf.len());
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| invalid("image lock poisoned"))?;
        inner.seek(SeekFrom::Start(offset))?;
        let mut done = 0;
        while done < want {
            let read = inner.read(&mut buf[done..want])?;
            if read == 0 {
                break;
            }
            done += read;
        }
        Ok(done)
    }

    fn read_vec(&self, offset: u64, len: usize) -> io::Result<Vec<u8>> {
        let mut data = vec![0_u8; len];
        let read = self.read_some(offset, &mut data)?;
        if read < len {
            return Err(invalid("the image ends inside a structure"));
        }
        Ok(data)
    }

    fn read_node(&self, node: &Node, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        if offset >= node.size || buf.is_empty() {
            return Ok(0);
        }
        let want = usize::try_from((node.size - offset).min(buf.len() as u64)).unwrap_or(buf.len());
        match &node.data {
            Data::Inline(bytes) => {
                let from = usize::try_from(offset)
                    .unwrap_or(usize::MAX)
                    .min(bytes.len());
                let count = want.min(bytes.len() - from);
                buf[..count].copy_from_slice(&bytes[from..from + count]);
                Ok(count)
            }
            Data::Extents(extents) => {
                let mut position = 0_u64;
                for extent in extents {
                    if offset < position + extent.len {
                        let inside = offset - position;
                        let count =
                            want.min(usize::try_from(extent.len - inside).unwrap_or(usize::MAX));
                        return match extent.start {
                            Some(start) => self.read_some(start + inside, &mut buf[..count]),
                            None => {
                                buf[..count].fill(0);
                                Ok(count)
                            }
                        };
                    }
                    position += extent.len;
                }
                Ok(0)
            }
        }
    }

    fn read_all(&self, node: &Node) -> io::Result<Vec<u8>> {
        if node.size > MAX_DIRECTORY {
            return Err(invalid("a directory is implausibly large"));
        }
        let mut data = vec![0_u8; usize::try_from(node.size).unwrap_or(0)];
        let mut done = 0;
        while done < data.len() {
            let read = self.read_node(node, done as u64, &mut data[done..])?;
            if read == 0 {
                return Err(invalid("a directory is shorter than it says"));
            }
            done += read;
        }
        Ok(data)
    }
}

/// A disc image. Open it with [`IsoImage::open`], then use it as a [`DiscFs`].
pub struct IsoImage<R> {
    source: Source<R>,
    flavor: Flavor,
    root: Node,
}

enum Flavor {
    Iso9660 { block: u64 },
    Udf(Udf),
}

impl<R: Read + Seek> IsoImage<R> {
    /// Opens an image, trying UDF first and ISO 9660 second.
    ///
    /// # Errors
    /// `InvalidData` when the bytes are neither (or use a feature this reader lacks), with the
    /// reason; any I/O error of the reader.
    pub fn open(mut reader: R) -> io::Result<Self> {
        let len = reader.seek(SeekFrom::End(0))?;
        let source = Source {
            inner: Mutex::new(reader),
            len,
        };
        let udf_problem = match Udf::open(&source) {
            Ok((udf, root)) => {
                return Ok(Self {
                    source,
                    flavor: Flavor::Udf(udf),
                    root,
                });
            }
            Err(e) => e,
        };
        match open_iso9660(&source) {
            Ok((block, root)) => Ok(Self {
                source,
                flavor: Flavor::Iso9660 { block },
                root,
            }),
            Err(iso_problem) => Err(invalid(format!(
                "not a readable disc image (UDF: {udf_problem}; ISO 9660: {iso_problem})"
            ))),
        }
    }

    /// Which file system was used: `"UDF"` or `"ISO 9660"`.
    #[must_use]
    pub const fn file_system(&self) -> &'static str {
        match self.flavor {
            Flavor::Udf(_) => "UDF",
            Flavor::Iso9660 { .. } => "ISO 9660",
        }
    }

    fn children(&self, dir: &Node) -> io::Result<Vec<(String, Node)>> {
        let data = self.source.read_all(dir)?;
        match &self.flavor {
            Flavor::Iso9660 { block } => Ok(iso9660_children(&data, *block)),
            Flavor::Udf(udf) => Ok(udf.children(&self.source, &data)),
        }
    }

    fn lookup(&self, path: &str) -> io::Result<Node> {
        let mut node = self.root.clone();
        let parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
        if parts.len() > MAX_DEPTH {
            return Err(not_found(path));
        }
        for part in parts {
            if !node.is_dir {
                return Err(not_found(path));
            }
            let found = self
                .children(&node)?
                .into_iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(part));
            match found {
                Some((_, child)) => node = child,
                None => return Err(not_found(path)),
            }
        }
        Ok(node)
    }
}

impl<R: Read + Seek> DiscFs for IsoImage<R> {
    fn list(&self, dir: &str) -> io::Result<Vec<DiscEntry>> {
        let node = self.lookup(dir)?;
        if !node.is_dir {
            return Err(invalid(format!("{dir} is not a directory")));
        }
        Ok(self
            .children(&node)?
            .into_iter()
            .map(|(name, child)| DiscEntry {
                name,
                is_dir: child.is_dir,
                size: if child.is_dir { 0 } else { child.size },
            })
            .collect())
    }

    fn size(&self, path: &str) -> io::Result<u64> {
        Ok(self.lookup(path)?.size)
    }

    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        let node = self.lookup(path)?;
        // A read may span several extents; fill the buffer unless the file ends first.
        let mut done = 0;
        while done < buf.len() {
            let read = self
                .source
                .read_node(&node, offset + done as u64, &mut buf[done..])?;
            if read == 0 {
                break;
            }
            done += read;
        }
        Ok(done)
    }
}

// ---------------------------------------------------------------------------------------------
// ISO 9660
// ---------------------------------------------------------------------------------------------

fn open_iso9660<R: Read + Seek>(source: &Source<R>) -> io::Result<(u64, Node)> {
    let descriptor = source.read_vec(16 * SECTOR, SECTOR as usize)?;
    if descriptor[0] != 1 || &descriptor[1..6] != b"CD001" {
        return Err(invalid("no ISO 9660 volume descriptor at sector 16"));
    }
    let block = u64::from(le16(&descriptor, 128));
    if block != SECTOR {
        return Err(invalid(format!(
            "logical block size {block} is not supported"
        )));
    }
    let record = &descriptor[156..190];
    let extent = u64::from(le32(record, 2)) * block;
    let size = u64::from(le32(record, 10));
    Ok((
        block,
        Node {
            is_dir: true,
            size,
            data: Data::Extents(vec![Extent {
                start: Some(extent),
                len: size,
            }]),
        },
    ))
}

fn iso9660_children(data: &[u8], block: u64) -> Vec<(String, Node)> {
    let mut out: Vec<(String, Node)> = Vec::new();
    let mut continues = false;
    let mut position = 0_usize;
    while position < data.len() {
        let length = usize::from(data[position]);
        if length == 0 {
            // Records never cross a sector; the rest of this one is padding.
            position = (position / SECTOR as usize + 1) * SECTOR as usize;
            continue;
        }
        if length < 34 || position + length > data.len() {
            break;
        }
        let record = &data[position..position + length];
        position += length;
        let name_length = usize::from(record[32]);
        if 33 + name_length > record.len() {
            break;
        }
        let raw_name = &record[33..33 + name_length];
        if raw_name == [0] || raw_name == [1] {
            continue;
        }
        let flags = record[25];
        let start = (u64::from(le32(record, 2)) + u64::from(record[1])) * block;
        let size = u64::from(le32(record, 10));
        let extent = Extent {
            start: Some(start),
            len: size,
        };
        match out.last_mut() {
            // A file over 4 GiB is several records with the same name; all but the last are
            // flagged as continuing.
            Some((_, node)) if continues => {
                if let Data::Extents(list) = &mut node.data {
                    list.push(extent);
                }
                node.size += size;
            }
            _ => {
                let name = String::from_utf8_lossy(raw_name).into_owned();
                let name = name
                    .split(';')
                    .next()
                    .unwrap_or("")
                    .trim_end_matches('.')
                    .to_owned();
                out.push((
                    name,
                    Node {
                        is_dir: flags & 0x02 != 0,
                        size,
                        data: Data::Extents(vec![extent]),
                    },
                ));
            }
        }
        continues = flags & 0x80 != 0;
    }
    out
}

// ---------------------------------------------------------------------------------------------
// UDF
// ---------------------------------------------------------------------------------------------

const TAG_AVDP: u16 = 2;
const TAG_VDP: u16 = 3;
const TAG_PD: u16 = 5;
const TAG_LVD: u16 = 6;
const TAG_TD: u16 = 8;
const TAG_FSD: u16 = 256;
const TAG_FID: u16 = 257;
const TAG_AED: u16 = 258;
const TAG_FE: u16 = 261;
const TAG_EFE: u16 = 266;
const FILE_TYPE_DIRECTORY: u8 = 4;

/// One entry of the logical volume's partition map.
#[derive(Debug)]
enum PartMap {
    /// A type 1 (or sparable, without remapping) partition.
    Physical {
        number: u16,
        start: u64,
    },
    /// A UDF 2.50 metadata partition: its blocks are the extents of the metadata file, which
    /// lives in the physical partition at `physical`.
    Metadata {
        physical: usize,
        /// `(byte offset inside the metadata partition, where those bytes are)`.
        extents: Vec<(u64, Extent)>,
    },
    Unsupported(&'static str),
}

struct Udf {
    maps: Vec<PartMap>,
}

fn tag_id(block: &[u8]) -> u16 {
    le16(block, 0)
}

impl Udf {
    fn open<R: Read + Seek>(source: &Source<R>) -> io::Result<(Self, Node)> {
        let last_sector = (source.len / SECTOR).saturating_sub(1);
        let anchor = [256, last_sector.saturating_sub(256), last_sector]
            .into_iter()
            .find_map(|sector| {
                let block = source.read_vec(sector * SECTOR, SECTOR as usize).ok()?;
                (tag_id(&block) == TAG_AVDP).then_some(block)
            })
            .ok_or_else(|| invalid("no UDF anchor volume descriptor pointer"))?;

        // The main sequence first, the reserve copy when the main one is unusable.
        let sequence = [
            (le32(&anchor, 16), le32(&anchor, 20)),
            (le32(&anchor, 24), le32(&anchor, 28)),
        ];
        let mut last_problem = invalid("empty volume descriptor sequence");
        for (length, location) in sequence {
            match Self::open_sequence(source, u64::from(length), u64::from(location)) {
                Ok(opened) => return Ok(opened),
                Err(e) => last_problem = e,
            }
        }
        Err(last_problem)
    }

    fn open_sequence<R: Read + Seek>(
        source: &Source<R>,
        mut length: u64,
        mut location: u64,
    ) -> io::Result<(Self, Node)> {
        let mut partitions: Vec<(u16, u64)> = Vec::new();
        let mut volume: Option<Vec<u8>> = None;
        let mut hops = 0;
        let mut index = 0;
        while index * SECTOR < length && index < 4096 {
            let block = source.read_vec((location + index) * SECTOR, SECTOR as usize)?;
            index += 1;
            match tag_id(&block) {
                TAG_PD => partitions.push((le16(&block, 22), u64::from(le32(&block, 188)))),
                TAG_LVD => volume = Some(block),
                TAG_VDP if hops < 8 => {
                    hops += 1;
                    length = u64::from(le32(&block, 20));
                    location = u64::from(le32(&block, 24));
                    index = 0;
                }
                TAG_TD => break,
                _ => {}
            }
        }
        let volume = volume.ok_or_else(|| invalid("no UDF logical volume descriptor"))?;
        if u64::from(le32(&volume, 212)) != SECTOR {
            return Err(invalid("UDF logical block size is not 2048"));
        }
        let table_length = usize::try_from(le32(&volume, 264)).unwrap_or(0);
        let count = usize::try_from(le32(&volume, 268)).unwrap_or(0);
        if 440 + table_length > volume.len() {
            return Err(invalid("UDF partition map table is too large"));
        }
        let table = &volume[440..440 + table_length];

        // First every map that names a physical partition, then the metadata maps that point
        // at them.
        let find = |number: u16| {
            partitions
                .iter()
                .find(|(n, _)| *n == number)
                .map(|(_, start)| *start)
        };
        let physical = |number: u16| match find(number) {
            Some(start) => PartMap::Physical { number, start },
            None => PartMap::Unsupported("a partition map names a missing partition"),
        };
        let mut maps: Vec<PartMap> = Vec::new();
        let mut pending_metadata: Vec<(usize, u16, u32, u32)> = Vec::new();
        let mut at = 0;
        for _ in 0..count {
            if at + 2 > table.len() {
                return Err(invalid("UDF partition map table is cut short"));
            }
            let (kind, size) = (table[at], usize::from(table[at + 1]));
            if size < 2 || at + size > table.len() {
                return Err(invalid("UDF partition map is malformed"));
            }
            let map = &table[at..at + size];
            at += size;
            match kind {
                1 if size >= 6 => maps.push(physical(le16(map, 4))),
                2 if size >= 40 => {
                    let identifier = String::from_utf8_lossy(&map[5..28]).into_owned();
                    let number = le16(map, 38);
                    if identifier.contains("UDF Metadata Partition") && size >= 48 {
                        pending_metadata.push((maps.len(), number, le32(map, 40), le32(map, 44)));
                        maps.push(PartMap::Unsupported("metadata partition not resolved"));
                    } else if identifier.contains("UDF Sparable Partition") {
                        maps.push(physical(number));
                    } else {
                        maps.push(PartMap::Unsupported("virtual partitions are not supported"));
                    }
                }
                _ => maps.push(PartMap::Unsupported("unknown partition map type")),
            }
        }

        let mut udf = Self { maps };
        for (slot, number, main, mirror) in pending_metadata {
            let physical = udf
                .maps
                .iter()
                .position(|m| matches!(m, PartMap::Physical { number: n, .. } if *n == number))
                .ok_or_else(|| invalid("a metadata partition names a missing partition"))?;
            let node = udf
                .read_entry(source, physical, u64::from(main))
                .or_else(|_| udf.read_entry(source, physical, u64::from(mirror)))?;
            let Data::Extents(list) = node.data else {
                return Err(invalid("the UDF metadata file has no extents"));
            };
            let mut extents = Vec::new();
            let mut offset = 0;
            for extent in list {
                let len = extent.len;
                extents.push((offset, extent));
                offset += len;
            }
            udf.maps[slot] = PartMap::Metadata { physical, extents };
        }

        // The file set descriptor names the root directory. Both are long allocation
        // descriptors: length, block, partition reference.
        let fsd_part = usize::from(le16(&volume, 248 + 8));
        let fsd_lbn = u64::from(le32(&volume, 248 + 4));
        let fsd = source.read_vec(udf.locate(fsd_part, fsd_lbn)?, SECTOR as usize)?;
        if tag_id(&fsd) != TAG_FSD {
            return Err(invalid("no UDF file set descriptor"));
        }
        let root_part = usize::from(le16(&fsd, 400 + 8));
        let root_lbn = u64::from(le32(&fsd, 400 + 4));
        let root = udf.read_entry(source, root_part, root_lbn)?;
        if !root.is_dir {
            return Err(invalid("the UDF root is not a directory"));
        }
        Ok((udf, root))
    }

    /// Where logical block `lbn` of partition reference `part` is, as a byte offset.
    fn locate(&self, part: usize, lbn: u64) -> io::Result<u64> {
        match self.maps.get(part) {
            Some(PartMap::Physical { start, .. }) => Ok((start + lbn) * SECTOR),
            Some(PartMap::Metadata { extents, .. }) => {
                let byte = lbn * SECTOR;
                extents
                    .iter()
                    .find(|(from, extent)| byte >= *from && byte < from + extent.len)
                    .and_then(|(from, extent)| extent.start.map(|start| start + (byte - from)))
                    .ok_or_else(|| invalid("a block lies outside the UDF metadata partition"))
            }
            Some(PartMap::Unsupported(reason)) => Err(invalid(*reason)),
            None => Err(invalid("a UDF block names a partition that does not exist")),
        }
    }

    /// Reads the file entry at `(part, lbn)`.
    fn read_entry<R: Read + Seek>(
        &self,
        source: &Source<R>,
        part: usize,
        lbn: u64,
    ) -> io::Result<Node> {
        let block = source.read_vec(self.locate(part, lbn)?, SECTOR as usize)?;
        let (attributes_length, descriptors_length, descriptors_at) = match tag_id(&block) {
            TAG_FE => (le32(&block, 168), le32(&block, 172), 176_usize),
            TAG_EFE => (le32(&block, 208), le32(&block, 212), 216),
            _ => {
                return Err(invalid(
                    "a UDF directory entry does not point at a file entry",
                ));
            }
        };
        let size = le64(&block, 56);
        let file_type = block[27];
        let allocation = le16(&block, 34) & 7;
        let start =
            descriptors_at.saturating_add(usize::try_from(attributes_length).unwrap_or(usize::MAX));
        let end = start.saturating_add(usize::try_from(descriptors_length).unwrap_or(usize::MAX));
        if end > block.len() {
            return Err(invalid("a UDF file entry overruns its block"));
        }
        let descriptors = block[start..end].to_vec();
        let is_dir = file_type == FILE_TYPE_DIRECTORY;
        if allocation == 3 {
            let keep = usize::try_from(size).unwrap_or(0).min(descriptors.len());
            return Ok(Node {
                is_dir,
                size: keep as u64,
                data: Data::Inline(descriptors[..keep].to_vec()),
            });
        }
        // File data in a file entry that sits in the metadata partition is in the physical
        // partition behind it. Directories are metadata and stay in the metadata partition.
        let data_part = match self.maps.get(part) {
            Some(PartMap::Metadata { physical, .. }) if !is_dir => *physical,
            _ => part,
        };
        let extents =
            self.allocation_extents(source, descriptors, allocation, part, data_part, size)?;
        Ok(Node {
            is_dir,
            size,
            data: Data::Extents(extents),
        })
    }

    fn allocation_extents<R: Read + Seek>(
        &self,
        source: &Source<R>,
        mut descriptors: Vec<u8>,
        allocation: u16,
        entry_part: usize,
        data_part: usize,
        size: u64,
    ) -> io::Result<Vec<Extent>> {
        let step = match allocation {
            0 => 8,
            1 => 16,
            _ => {
                return Err(invalid(
                    "this UDF allocation descriptor type is not supported",
                ));
            }
        };
        let mut extents: Vec<Extent> = Vec::new();
        let mut covered = 0_u64;
        for _ in 0..4096 {
            let mut next: Option<(usize, u64)> = None;
            for descriptor in descriptors.chunks_exact(step) {
                let raw = le32(descriptor, 0);
                let length = u64::from(raw & 0x3FFF_FFFF);
                if length == 0 {
                    break;
                }
                let lbn = u64::from(le32(descriptor, 4));
                let part = if step == 16 {
                    usize::from(le16(descriptor, 8))
                } else {
                    data_part
                };
                match raw >> 30 {
                    0 => {
                        extents.push(Extent {
                            start: Some(self.locate(part, lbn)?),
                            len: length,
                        });
                        covered += length;
                    }
                    1 | 2 => {
                        extents.push(Extent {
                            start: None,
                            len: length,
                        });
                        covered += length;
                    }
                    _ => {
                        next = Some((if step == 16 { part } else { entry_part }, lbn));
                        break;
                    }
                }
            }
            let Some((part, lbn)) = next else { break };
            let block = source.read_vec(self.locate(part, lbn)?, SECTOR as usize)?;
            if tag_id(&block) != TAG_AED {
                return Err(invalid("a UDF allocation continuation is malformed"));
            }
            let length = usize::try_from(le32(&block, 20)).unwrap_or(usize::MAX);
            if length > block.len() - 24 {
                return Err(invalid("a UDF allocation continuation overruns its block"));
            }
            descriptors = block[24..24 + length].to_vec();
        }
        // The last extent may be longer than the file.
        if covered > size {
            let mut excess = covered - size;
            while excess > 0 {
                let Some(last) = extents.last_mut() else {
                    break;
                };
                if last.len > excess {
                    last.len -= excess;
                    break;
                }
                excess -= last.len;
                extents.pop();
            }
        }
        Ok(extents)
    }

    /// The entries of a directory whose bytes are `data`. An entry whose file entry cannot be
    /// read is left out rather than hiding its siblings.
    fn children<R: Read + Seek>(&self, source: &Source<R>, data: &[u8]) -> Vec<(String, Node)> {
        let mut out = Vec::new();
        let mut at = 0;
        while at + 38 <= data.len() {
            let fid = &data[at..];
            if tag_id(fid) != TAG_FID {
                break;
            }
            let characteristics = fid[18];
            let name_length = usize::from(fid[19]);
            let implementation_length = usize::from(le16(fid, 36));
            let total = (38 + implementation_length + name_length + 3) & !3;
            if at + total > data.len() {
                break;
            }
            let hidden_entry = characteristics & 0x04 != 0 || characteristics & 0x08 != 0;
            if !hidden_entry && name_length > 0 {
                let from = 38 + implementation_length;
                let name = decode_name(&fid[from..from + name_length]);
                let part = usize::from(le16(fid, 20 + 8));
                let lbn = u64::from(le32(fid, 20 + 4));
                if let Ok(node) = self.read_entry(source, part, lbn) {
                    out.push((name, node));
                }
            }
            at += total;
        }
        out
    }
}

/// A UDF file identifier: a compression id byte, then 8-bit or UTF-16BE characters.
fn decode_name(raw: &[u8]) -> String {
    match raw.first() {
        Some(8) => raw[1..].iter().map(|&b| char::from(b)).collect(),
        Some(16) => {
            let units: Vec<u16> = raw[1..]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
                .collect();
            String::from_utf16_lossy(&units)
        }
        _ => String::from_utf8_lossy(raw).into_owned(),
    }
}
