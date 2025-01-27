use compress_tools as ct;


pub fn parse_game_start_actual(game_start: &[u8]) -> slp_parser::SlpResult<slp_parser::GameStart> {
    use slp_parser::*;

    fn read_f32(bytes: &[u8], offset: usize) -> f32 { f32::from_be_bytes(bytes[offset..][..4].try_into().unwrap()) }
    fn read_u32(bytes: &[u8], offset: usize) -> u32 { u32::from_be_bytes(bytes[offset..][..4].try_into().unwrap()) }
    fn read_u16(bytes: &[u8], offset: usize) -> u16 { u16::from_be_bytes(bytes[offset..][..2].try_into().unwrap()) }
    fn read_u8 (bytes: &[u8], offset: usize) -> u8  {  u8::from_be_bytes(bytes[offset..][..1].try_into().unwrap()) }
    fn read_i32(bytes: &[u8], offset: usize) -> i32 { i32::from_be_bytes(bytes[offset..][..4].try_into().unwrap()) }
    fn read_i8 (bytes: &[u8], offset: usize) -> i8  {  i8::from_be_bytes(bytes[offset..][..1].try_into().unwrap()) }
    fn read_array<const SIZE: usize>(bytes: &[u8], offset: usize) -> SlpResult<[u8; SIZE]> {
        if bytes.len() < offset+SIZE { return Err(SlpError::IOError); }
        Ok(bytes[offset..][..SIZE].try_into().unwrap())
    }

    if game_start.len() < 5 { return Err(SlpError::InvalidFile(InvalidLocation::GameStart)); }
    if game_start[0] != 0x36 { return Err(SlpError::InvalidFile(InvalidLocation::GameStart)); }

    let version = read_array::<4>(game_start, 1)?;
    if version[0] != 3 { println!("too old"); return Err(SlpError::InvalidFile(InvalidLocation::GameStart)); }
    //if version[1] < 7 { println!("too old"); return Err(SlpError::InvalidFile(InvalidLocation::GameStart)); }

    //if game_start.len() < 761 { return Err(SlpError::InvalidFile(InvalidLocation::GameStart)); }
    let game_info_block = &game_start[5..];

    let stage = Stage::from_u16(read_u16(game_info_block, 0xE))
        .ok_or(SlpError::InvalidFile(InvalidLocation::GameStart))?;

    let timer = read_u32(game_info_block, 0x10);
    
    let mut starting_character_colours = [None; 4];
    let mut names = [[0u8; 31]; 4];
    let mut connect_codes = [[0u8; 10]; 4];

    for i in 0..4 {
        if read_u8(game_info_block, 0x61 + 0x24*i) == 3 { continue; }

        let character = Character::from_u8_external(read_u8(game_info_block, 0x60 + 0x24*i))
            .ok_or(SlpError::InvalidFile(InvalidLocation::GameStart))?;
        let character_colour = CharacterColour::from_character_and_colour(character, read_u8(game_info_block, 0x63 + 0x24*i))
            .ok_or(SlpError::InvalidFile(InvalidLocation::GameStart))?;

        starting_character_colours[i] = Some(character_colour);
        names[i] = read_array::<31>(game_start, 0x1A5 + 0x1F*i)?;
        connect_codes[i] = read_array::<10>(game_start, 0x221 + 0xA*i)?;
    }

    Ok(GameStart {
        stage,
        starting_character_colours,
        timer,
        names,
        connect_codes,
    })
}

fn check_game_start(buf: &[u8]) -> bool {
    let Ok(header) = slp_parser::parse_raw_header(&buf) else { return false };
    let Ok(sizes) = slp_parser::event_sizes(&buf, header.event_sizes_offset) else { return false; };
    let game_start_size = sizes.event_sizes[0x36 as usize] as usize + 1;
    if buf.len() < sizes.game_start_offset + game_start_size { return false; }

    let mut count = 0;
    for i in 0..4 {
        let typ = buf[sizes.game_start_offset + 0x66 + 0x24*i];
        if typ == 3 { continue; }
        if typ != 0 { return false; }
        count += 1;

        let char_ext = buf[sizes.game_start_offset + 0x65 + 0x24*i];
        if char_ext != 2 { return false; }
    }
    if count != 2 { return false; }

    true
}

fn handle_replay(
    compressor: &mut slpz::Compressor,
    buf: &mut Vec<u8>,
    filename: &str,
    replay_iter: &mut ct::ArchiveIterator<std::fs::File>,
) {
    println!("  decoding {}", filename);
    buf.clear();

    loop {
        match replay_iter.next() {
            Some(ct::ArchiveContents::DataChunk(bytes)) => buf.extend_from_slice(&bytes),
            Some(ct::ArchiveContents::EndOfEntry) => break,
            _ => {
                eprintln!("ERROR: expected DataChunk or EndOfEntry");
                return;
            }
        }
    }

    // parse game info 

    if !check_game_start(&buf) { return; }

    // no skip - fox ditto

    match slpz::compress(compressor, &buf) {
        Err(e) => {
            eprintln!("ERROR: could not compress: {}", e);
        },
        Ok(slpz_bytes) => {
            let mut output_path = std::path::Path::new("output/").join(filename);
            output_path.set_extension("slpz");

            match std::fs::write(output_path, slpz_bytes) {
                Ok(_) => println!("  wrote slpz to file"),
                Err(e) => eprintln!("ERROR: could not write file: {}", e),
            }
        }
    }
}

fn main() {
    let mut alloc = Vec::with_capacity(16 * 1024 * 1024);
    let mut compressor = slpz::Compressor::new(3).unwrap();

    for zip in std::fs::read_dir("input_zips").unwrap() {
        let zip = zip.unwrap();
        let zip_path = zip.path();
        println!("reading {}", zip_path.display());
        let f = std::fs::File::open(&zip_path).unwrap();

        let mut contents_iter = ct::ArchiveIteratorBuilder::new(f)
            //.filter(|name, _| &name[0..13] > "Game_20200700")
            .build()
            .unwrap();
        while let Some(contents) = contents_iter.next() {
            let ct::ArchiveContents::StartOfEntry(name, _) = contents else {
                eprintln!("ERROR: expected StartOfEntry");
                continue;
            };

            handle_replay(&mut compressor, &mut alloc, &name, &mut contents_iter)
        }
    }
}
