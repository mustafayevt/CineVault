//! Builders for the structure files of discs: DVD `.IFO` files and Blu-ray `.mpls` playlists.

fn bcd(value: u8) -> u8 {
    (value / 10) << 4 | (value % 10)
}

/// A DVD time as the IFO stores it: BCD `hh mm ss`, then the frame count with the frame rate
/// in the top two bits (3 is 29.97 fps, 1 is 25 fps).
pub fn dvd_time(hours: u8, minutes: u8, seconds: u8, frames: u8, rate_flag: u8) -> [u8; 4] {
    [
        bcd(hours),
        bcd(minutes),
        bcd(seconds),
        rate_flag << 6 | bcd(frames),
    ]
}

fn sector_pointer(ifo: &mut [u8], at: usize, sector: u32) {
    ifo[at..at + 4].copy_from_slice(&sector.to_be_bytes());
}

/// `VIDEO_TS.IFO` with a title table: `(chapters, title set, title number inside the set)`.
pub fn vmg_ifo(titles: &[(u16, u8, u8)]) -> Vec<u8> {
    let mut ifo = vec![0_u8; 2048 * 2];
    ifo[..12].copy_from_slice(b"DVDVIDEO-VMG");
    sector_pointer(&mut ifo, 0xC4, 1);
    let table = &mut ifo[2048..];
    table[0..2].copy_from_slice(&u16::try_from(titles.len()).unwrap().to_be_bytes());
    let end = 8 + titles.len() * 12 - 1;
    table[4..8].copy_from_slice(&u32::try_from(end).unwrap().to_be_bytes());
    for (index, (chapters, set, number)) in titles.iter().enumerate() {
        let at = 8 + index * 12;
        table[at] = 0x01;
        table[at + 1] = 1;
        table[at + 2..at + 4].copy_from_slice(&chapters.to_be_bytes());
        table[at + 6] = *set;
        table[at + 7] = *number;
        table[at + 8..at + 12].copy_from_slice(&(index as u32 + 100).to_be_bytes());
    }
    ifo
}

/// `VTS_nn_0.IFO` with program chains (by playback time) and titles. A title is the number of
/// the chain it plays (1-based) and its chapter count; every chapter is one program.
pub fn vts_ifo(chains: &[[u8; 4]], titles: &[(u16, usize)]) -> Vec<u8> {
    let mut ifo = vec![0_u8; 2048 * 3];
    ifo[..12].copy_from_slice(b"DVDVIDEO-VTS");
    sector_pointer(&mut ifo, 0xC8, 1);
    sector_pointer(&mut ifo, 0xCC, 2);

    // Part-of-title search pointers: a header, one offset per title, then (chain, program)
    // pairs.
    let ptt = &mut ifo[2048..4096];
    ptt[0..2].copy_from_slice(&u16::try_from(titles.len()).unwrap().to_be_bytes());
    let mut at = 8 + titles.len() * 4;
    for (index, (chain, chapters)) in titles.iter().enumerate() {
        ptt[8 + index * 4..12 + index * 4]
            .copy_from_slice(&u32::try_from(at).unwrap().to_be_bytes());
        for program in 1..=*chapters {
            ptt[at..at + 2].copy_from_slice(&chain.to_be_bytes());
            ptt[at + 2..at + 4].copy_from_slice(&u16::try_from(program).unwrap().to_be_bytes());
            at += 4;
        }
    }
    ptt[4..8].copy_from_slice(&u32::try_from(at - 1).unwrap().to_be_bytes());

    // Program chain table: a header, one search entry per chain, then the chains.
    let pgcit = &mut ifo[4096..];
    pgcit[0..2].copy_from_slice(&u16::try_from(chains.len()).unwrap().to_be_bytes());
    let mut at = 8 + chains.len() * 8;
    for (index, time) in chains.iter().enumerate() {
        pgcit[8 + index * 8..12 + index * 8].copy_from_slice(&0x8100_0000_u32.to_be_bytes());
        pgcit[12 + index * 8..16 + index * 8]
            .copy_from_slice(&u32::try_from(at).unwrap().to_be_bytes());
        pgcit[at + 2] = 1;
        pgcit[at + 3] = 1;
        pgcit[at + 4..at + 8].copy_from_slice(time);
        at += 0xEC;
    }
    pgcit[4..8].copy_from_slice(&u32::try_from(at - 1).unwrap().to_be_bytes());
    ifo
}

/// One play item of a playlist: clip name, in and out time (45 kHz ticks).
pub type Item<'a> = (&'a str, u32, u32);

/// A playlist file. `marks` are the chapter marks' types (1 is an entry mark).
pub fn mpls(items: &[Item<'_>], marks: &[u8]) -> Vec<u8> {
    let mut list = Vec::new();
    list.extend_from_slice(&0_u32.to_be_bytes()); // length, set below
    list.extend_from_slice(&0_u16.to_be_bytes());
    list.extend_from_slice(&u16::try_from(items.len()).unwrap().to_be_bytes());
    list.extend_from_slice(&0_u16.to_be_bytes());
    for (clip, start, end) in items {
        let mut body = Vec::new();
        body.extend_from_slice(clip.as_bytes());
        body.extend_from_slice(b"M2TS");
        body.extend_from_slice(&1_u16.to_be_bytes()); // connection condition 1
        body.push(0); // STC
        body.extend_from_slice(&start.to_be_bytes());
        body.extend_from_slice(&end.to_be_bytes());
        body.extend_from_slice(&[0; 8]); // user operation mask
        body.extend_from_slice(&[0, 0, 0, 0]); // flags, still mode, still time
        list.extend_from_slice(&u16::try_from(body.len()).unwrap().to_be_bytes());
        list.extend(body);
    }
    let length = u32::try_from(list.len() - 4).unwrap();
    list[0..4].copy_from_slice(&length.to_be_bytes());

    let mut marks_section = Vec::new();
    marks_section.extend_from_slice(&0_u32.to_be_bytes());
    marks_section.extend_from_slice(&u16::try_from(marks.len()).unwrap().to_be_bytes());
    for (index, kind) in marks.iter().enumerate() {
        let mut mark = [0_u8; 14];
        mark[1] = *kind;
        mark[2..4].copy_from_slice(&0_u16.to_be_bytes());
        mark[4..8].copy_from_slice(&(index as u32 * 45_000).to_be_bytes());
        mark[8..10].copy_from_slice(&0xFFFF_u16.to_be_bytes());
        marks_section.extend_from_slice(&mark);
    }
    let marks_length = u32::try_from(marks_section.len() - 4).unwrap();
    marks_section[0..4].copy_from_slice(&marks_length.to_be_bytes());

    let list_at = 40_u32;
    let marks_at = list_at + u32::try_from(list.len()).unwrap();
    let mut file = Vec::new();
    file.extend_from_slice(b"MPLS0200");
    file.extend_from_slice(&list_at.to_be_bytes());
    file.extend_from_slice(&marks_at.to_be_bytes());
    file.extend_from_slice(&0_u32.to_be_bytes());
    file.resize(40, 0);
    file.extend(list);
    file.extend(marks_section);
    file
}
