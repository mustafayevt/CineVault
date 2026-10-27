//! Builders for disc images used by the disc tests: ISO 9660, and UDF with or without a
//! metadata partition (UDF 1.02 style and UDF 2.50 style). They write only what a reader needs
//! (no path tables, no descriptor CRCs), laid out the way mastering tools lay discs out.

use std::collections::BTreeMap;

const SECTOR: usize = 2048;

/// Files by `/`-separated path. Directories are implied.
pub type Files<'a> = Vec<(&'a str, Vec<u8>)>;

/// A directory tree built from paths.
#[derive(Default)]
struct Dir {
    dirs: BTreeMap<String, Dir>,
    files: BTreeMap<String, Vec<u8>>,
}

impl Dir {
    fn from_files(files: &[(&str, Vec<u8>)]) -> Self {
        let mut root = Self::default();
        for (path, data) in files {
            let mut parts: Vec<&str> = path.split('/').collect();
            let name = parts.pop().expect("file name");
            let mut dir = &mut root;
            for part in parts {
                dir = dir.dirs.entry(part.to_owned()).or_default();
            }
            dir.files.insert(name.to_owned(), data.clone());
        }
        root
    }
}

fn blocks_for(len: usize) -> usize {
    len.div_ceil(SECTOR)
}

fn put16(buf: &mut [u8], at: usize, value: u16) {
    buf[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

fn put32(buf: &mut [u8], at: usize, value: u32) {
    buf[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn put64(buf: &mut [u8], at: usize, value: u64) {
    buf[at..at + 8].copy_from_slice(&value.to_le_bytes());
}

// ---------------------------------------------------------------------------------------------
// ISO 9660
// ---------------------------------------------------------------------------------------------

fn iso_record(name: &[u8], extent: u32, size: u32, is_dir: bool) -> Vec<u8> {
    let mut length = 33 + name.len();
    length += length % 2;
    let mut record = vec![0_u8; length];
    record[0] = u8::try_from(length).expect("record length");
    record[2..6].copy_from_slice(&extent.to_le_bytes());
    record[6..10].copy_from_slice(&extent.to_be_bytes());
    record[10..14].copy_from_slice(&size.to_le_bytes());
    record[14..18].copy_from_slice(&size.to_be_bytes());
    record[25] = if is_dir { 2 } else { 0 };
    record[28..30].copy_from_slice(&1_u16.to_le_bytes());
    record[30..32].copy_from_slice(&1_u16.to_be_bytes());
    record[32] = u8::try_from(name.len()).expect("name length");
    record[33..33 + name.len()].copy_from_slice(name);
    record
}

/// An ISO 9660 image of `files`, with upper-case names and `;1` versions like a mastering tool
/// writes them. Each directory must fit one sector.
pub fn iso9660(files: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let root = Dir::from_files(files);

    // Sectors: 16 volume descriptor, 17 terminator, 18.. directories, then file data.
    fn count_dirs(dir: &Dir) -> usize {
        1 + dir.dirs.values().map(count_dirs).sum::<usize>()
    }
    let directory_count = count_dirs(&root);
    let mut next_directory = 19_usize;
    let mut next_data = 18 + directory_count;
    let mut sectors: BTreeMap<usize, Vec<u8>> = BTreeMap::new();

    fn emit(
        dir: &Dir,
        parent: usize,
        own: usize,
        next_directory: &mut usize,
        next_data: &mut usize,
        sectors: &mut BTreeMap<usize, Vec<u8>>,
    ) {
        let mut content = Vec::new();
        content.extend(iso_record(
            &[0],
            u32::try_from(own).unwrap(),
            SECTOR as u32,
            true,
        ));
        content.extend(iso_record(
            &[1],
            u32::try_from(parent).unwrap(),
            SECTOR as u32,
            true,
        ));
        for (name, child) in &dir.dirs {
            let at = *next_directory;
            *next_directory += 1;
            content.extend(iso_record(
                name.to_uppercase().as_bytes(),
                u32::try_from(at).unwrap(),
                SECTOR as u32,
                true,
            ));
            emit(child, own, at, next_directory, next_data, sectors);
        }
        for (name, data) in &dir.files {
            let at = *next_data;
            *next_data += blocks_for(data.len()).max(1);
            for (index, chunk) in data.chunks(SECTOR).enumerate() {
                sectors.insert(at + index, chunk.to_vec());
            }
            let label = format!("{};1", name.to_uppercase());
            content.extend(iso_record(
                label.as_bytes(),
                u32::try_from(at).unwrap(),
                u32::try_from(data.len()).unwrap(),
                false,
            ));
        }
        assert!(content.len() <= SECTOR, "test directory too large");
        sectors.insert(own, content);
    }
    emit(
        &root,
        18,
        18,
        &mut next_directory,
        &mut next_data,
        &mut sectors,
    );

    let total = next_data;
    let mut image = vec![0_u8; total * SECTOR];
    let pvd = &mut image[16 * SECTOR..17 * SECTOR];
    pvd[0] = 1;
    pvd[1..6].copy_from_slice(b"CD001");
    pvd[6] = 1;
    pvd[8..40].fill(b' ');
    pvd[40..72].fill(b' ');
    pvd[40..48].copy_from_slice(b"TESTDISC");
    put32(pvd, 80, u32::try_from(total).unwrap());
    pvd[84..88].copy_from_slice(&u32::try_from(total).unwrap().to_be_bytes());
    put16(pvd, 120, 1);
    pvd[122..124].copy_from_slice(&1_u16.to_be_bytes());
    put16(pvd, 124, 1);
    pvd[126..128].copy_from_slice(&1_u16.to_be_bytes());
    put16(pvd, 128, SECTOR as u16);
    pvd[130..132].copy_from_slice(&(SECTOR as u16).to_be_bytes());
    let root_record = iso_record(&[0], 18, SECTOR as u32, true);
    pvd[156..156 + root_record.len()].copy_from_slice(&root_record);
    let terminator = &mut image[17 * SECTOR..18 * SECTOR];
    terminator[0] = 255;
    terminator[1..6].copy_from_slice(b"CD001");
    terminator[6] = 1;
    for (sector, data) in sectors {
        image[sector * SECTOR..sector * SECTOR + data.len()].copy_from_slice(&data);
    }
    image
}

// ---------------------------------------------------------------------------------------------
// UDF
// ---------------------------------------------------------------------------------------------

/// Which UDF flavour to write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UdfFlavor {
    /// One physical partition, file entries (UDF 1.02, DVD style).
    Plain,
    /// A metadata partition holding the extended file entries and directories (UDF 2.50, the
    /// Blu-ray style); file data stays in the physical partition.
    Metadata,
}

/// The partition starts here, leaving room for the volume structures and the anchor at 256.
const PARTITION_START: usize = 288;
/// Files this small are stored inside their file entry.
const EMBED_LIMIT: usize = 200;

fn tag(block: &mut [u8], id: u16, location: u32) {
    put16(block, 0, id);
    put16(block, 2, 3);
    put16(block, 6, 1);
    put32(block, 12, location);
    let sum = block[..16]
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != 4)
        .fold(0_u8, |sum, (_, byte)| sum.wrapping_add(*byte));
    block[4] = sum;
}

fn long_ad(block: &mut [u8], at: usize, length: u32, lbn: u32, partition: u16) {
    put32(block, at, length);
    put32(block, at + 4, lbn);
    put16(block, at + 8, partition);
}

/// What a file entry points at.
enum Content {
    Embedded(Vec<u8>),
    /// `(length, logical block)` extents.
    Extents(Vec<(u32, u32)>),
}

fn file_entry(extended: bool, file_type: u8, size: u64, content: &Content) -> Vec<u8> {
    let mut block = vec![0_u8; SECTOR];
    let (descriptors_at, length_at) = if extended { (216, 212) } else { (176, 172) };
    tag(&mut block, if extended { 266 } else { 261 }, 0);
    block[27] = file_type;
    put64(&mut block, 56, size);
    match content {
        Content::Embedded(bytes) => {
            put16(&mut block, 34, 3);
            put32(&mut block, length_at, u32::try_from(bytes.len()).unwrap());
            block[descriptors_at..descriptors_at + bytes.len()].copy_from_slice(bytes);
        }
        Content::Extents(extents) => {
            put16(&mut block, 34, 0);
            put32(
                &mut block,
                length_at,
                u32::try_from(extents.len() * 8).unwrap(),
            );
            for (index, (length, lbn)) in extents.iter().enumerate() {
                let at = descriptors_at + index * 8;
                put32(&mut block, at, *length);
                put32(&mut block, at + 4, *lbn);
            }
        }
    }
    block
}

fn file_identifier(
    name: &str,
    is_dir: bool,
    parent: bool,
    icb_lbn: u32,
    partition: u16,
) -> Vec<u8> {
    let encoded: Vec<u8> = if parent {
        Vec::new()
    } else if name.is_ascii() {
        std::iter::once(8).chain(name.bytes()).collect()
    } else {
        std::iter::once(16)
            .chain(name.encode_utf16().flat_map(u16::to_be_bytes))
            .collect()
    };
    let total = (38 + encoded.len() + 3) & !3;
    let mut fid = vec![0_u8; total];
    tag(&mut fid, 257, 0);
    put16(&mut fid, 16, 1);
    fid[18] = if parent {
        0x0A
    } else if is_dir {
        0x02
    } else {
        0
    };
    fid[19] = u8::try_from(encoded.len()).unwrap();
    long_ad(&mut fid, 20, SECTOR as u32, icb_lbn, partition);
    fid[38..38 + encoded.len()].copy_from_slice(&encoded);
    fid
}

/// One allocated node of the tree.
struct Placed {
    /// Block of the file entry, in the metadata area.
    entry: u32,
    /// Block of a directory's content, in the metadata area.
    content: u32,
}

/// Where the builder put the nodes of a tree, and how blocks are addressed.
struct Layout {
    extended: bool,
    /// Physical block where the metadata area starts.
    metadata_start: u32,
    /// The partition reference directory entries use: the metadata partition or the physical.
    directory_partition: u16,
    placements: BTreeMap<String, Placed>,
}

impl Layout {
    /// Where metadata block `block` really is in the physical partition.
    fn physical(&self, block: u32) -> u32 {
        self.metadata_start + block
    }
}

/// Gives every node a metadata block, depth first. Blocks 0 to 2 are the file set descriptor
/// and the root's entry and content.
fn allocate(dir: &Dir, prefix: &str, count: &mut u32, placements: &mut BTreeMap<String, Placed>) {
    for (name, child) in &dir.dirs {
        let path = format!("{prefix}{name}/");
        placements.insert(
            path.clone(),
            Placed {
                entry: *count,
                content: *count + 1,
            },
        );
        *count += 2;
        allocate(child, &path, count, placements);
    }
    for name in dir.files.keys() {
        placements.insert(
            format!("{prefix}{name}"),
            Placed {
                entry: *count,
                content: 0,
            },
        );
        *count += 1;
    }
}

/// Writes one directory and everything under it into `blocks` (by physical block).
fn fill(
    dir: &Dir,
    prefix: &str,
    parent_entry: u32,
    own: (u32, u32),
    layout: &Layout,
    next_data: &mut u32,
    blocks: &mut BTreeMap<u32, Vec<u8>>,
) {
    let part = layout.directory_partition;
    let mut content = file_identifier("", true, true, parent_entry, part);
    for (name, child) in &dir.dirs {
        let placed = &layout.placements[&format!("{prefix}{name}/")];
        content.extend(file_identifier(name, true, false, placed.entry, part));
        fill(
            child,
            &format!("{prefix}{name}/"),
            own.0,
            (placed.entry, placed.content),
            layout,
            next_data,
            blocks,
        );
    }
    for (name, data) in &dir.files {
        let placed = &layout.placements[&format!("{prefix}{name}")];
        content.extend(file_identifier(name, false, false, placed.entry, part));
        let body = if data.len() <= EMBED_LIMIT {
            Content::Embedded(data.clone())
        } else {
            let start = *next_data;
            let total = blocks_for(data.len());
            *next_data += u32::try_from(total).unwrap();
            for (index, chunk) in data.chunks(SECTOR).enumerate() {
                blocks.insert(start + u32::try_from(index).unwrap(), chunk.to_vec());
            }
            let length = u32::try_from(data.len()).unwrap();
            if total > 2 {
                // Larger files get two extents, to exercise offsets across extents.
                let first = SECTOR as u32;
                Content::Extents(vec![(first, start), (length - first, start + 1)])
            } else {
                Content::Extents(vec![(length, start)])
            }
        };
        let entry = file_entry(layout.extended, 5, data.len() as u64, &body);
        blocks.insert(layout.physical(placed.entry), entry);
    }
    assert!(content.len() <= SECTOR, "test directory too large");
    let size = content.len() as u64;
    let extents = Content::Extents(vec![(u32::try_from(size).unwrap(), own.1)]);
    blocks.insert(
        layout.physical(own.0),
        file_entry(layout.extended, 4, size, &extents),
    );
    blocks.insert(layout.physical(own.1), content);
}

/// A UDF image of `files`. Each directory must fit one block.
pub fn udf(files: &[(&str, Vec<u8>)], flavor: UdfFlavor) -> Vec<u8> {
    let root = Dir::from_files(files);
    let metadata = flavor == UdfFlavor::Metadata;

    let mut placements = BTreeMap::new();
    let mut metadata_blocks = 3_u32;
    allocate(&root, "", &mut metadata_blocks, &mut placements);

    // Physical partition layout: with a metadata partition, the metadata file and its mirror
    // come first, then the metadata area, then file data; without, the area is first.
    let (metadata_start, data_start) = if metadata {
        (2_u32, 2 + metadata_blocks)
    } else {
        (0, metadata_blocks)
    };
    let directory_partition = u16::from(metadata);
    let layout = Layout {
        extended: metadata,
        metadata_start,
        directory_partition,
        placements,
    };

    let mut blocks: BTreeMap<u32, Vec<u8>> = BTreeMap::new();
    let mut next_data = data_start;
    fill(&root, "", 1, (1, 2), &layout, &mut next_data, &mut blocks);
    let ctx = &layout;

    // File set descriptor (metadata block 0).
    let mut fsd = vec![0_u8; SECTOR];
    tag(&mut fsd, 256, 0);
    long_ad(&mut fsd, 400, SECTOR as u32, 1, directory_partition);
    blocks.insert(ctx.physical(0), fsd);

    if metadata {
        // Metadata file and its mirror: one extent over the whole metadata area.
        let area = metadata_blocks * SECTOR as u32;
        let file = file_entry(
            true,
            250,
            u64::from(area),
            &Content::Extents(vec![(area, metadata_start)]),
        );
        blocks.insert(0, file.clone());
        blocks.insert(1, file);
    }

    let partition_blocks = next_data.max(data_start);
    let total_sectors = PARTITION_START + partition_blocks as usize;
    let mut image = vec![0_u8; total_sectors * SECTOR];

    // Volume recognition sequence.
    for (sector, id) in [(16, b"BEA01"), (17, b"NSR02"), (18, b"TEA01")] {
        image[sector * SECTOR + 1..sector * SECTOR + 6].copy_from_slice(id);
        image[sector * SECTOR] = 0;
        image[sector * SECTOR + 6] = 1;
    }

    // Volume descriptor sequence, twice (main at 32, reserve at 48).
    for base in [32_usize, 48] {
        let mut pd = vec![0_u8; SECTOR];
        tag(&mut pd, 5, u32::try_from(base + 1).unwrap());
        put16(&mut pd, 22, 0);
        put32(&mut pd, 188, PARTITION_START as u32);
        put32(&mut pd, 192, partition_blocks);

        let mut lvd = vec![0_u8; SECTOR];
        tag(&mut lvd, 6, u32::try_from(base + 2).unwrap());
        put32(&mut lvd, 212, SECTOR as u32);
        long_ad(&mut lvd, 248, SECTOR as u32, 0, directory_partition);
        let mut maps = vec![1_u8, 6, 1, 0, 0, 0];
        let mut map_count = 1;
        if metadata {
            let mut map = vec![0_u8; 64];
            map[0] = 2;
            map[1] = 64;
            let identifier = b"*UDF Metadata Partition";
            map[5..5 + identifier.len()].copy_from_slice(identifier);
            put16(&mut map, 36, 1);
            put16(&mut map, 38, 0);
            put32(&mut map, 40, 0);
            put32(&mut map, 44, 1);
            put32(&mut map, 48, u32::MAX);
            put32(&mut map, 52, 1);
            put16(&mut map, 56, 1);
            maps.extend(map);
            map_count = 2;
        }
        put32(&mut lvd, 264, u32::try_from(maps.len()).unwrap());
        put32(&mut lvd, 268, map_count);
        lvd[440..440 + maps.len()].copy_from_slice(&maps);

        let mut terminator = vec![0_u8; SECTOR];
        tag(&mut terminator, 8, u32::try_from(base + 3).unwrap());
        for (offset, block) in [(1, pd), (2, lvd), (3, terminator)] {
            let at = (base + offset) * SECTOR;
            image[at..at + SECTOR].copy_from_slice(&block);
        }
    }

    // Anchor volume descriptor pointer at sector 256.
    let mut anchor = vec![0_u8; SECTOR];
    tag(&mut anchor, 2, 256);
    put32(&mut anchor, 16, 16 * SECTOR as u32);
    put32(&mut anchor, 20, 32);
    put32(&mut anchor, 24, 16 * SECTOR as u32);
    put32(&mut anchor, 28, 48);
    image[256 * SECTOR..257 * SECTOR].copy_from_slice(&anchor);

    for (block, data) in blocks {
        let at = (PARTITION_START + block as usize) * SECTOR;
        image[at..at + data.len()].copy_from_slice(&data);
    }
    image
}
