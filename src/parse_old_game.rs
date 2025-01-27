use slp_parser::*;

const EVENT_PAYLOADS:       u8 = 0x35;
const GAME_START:           u8 = 0x36;
const PRE_FRAME_UPDATE:     u8 = 0x37;
const POST_FRAME_UPDATE:    u8 = 0x38;
const GAME_END:             u8 = 0x39;
//const FRAME_START:          u8 = 0x3A;
const ITEM_UPDATE:          u8 = 0x3B;
const FRAME_BOOKEND:        u8 = 0x3C;
//const GECKO_LIST:           u8 = 0x3D;

pub const MAX_SUPPORTED_SLPZ_VERSION: u32 = 0;

pub const MIN_VERSION_MAJOR: u8 = 1;
pub const MIN_VERSION_MINOR: u8 = 0;

pub const HEADER_LEN: u64 = 15;

fn read_f32(bytes: &[u8], offset: usize) -> f32 { f32::from_be_bytes(bytes[offset..][..4].try_into().unwrap()) }
fn read_u32(bytes: &[u8], offset: usize) -> u32 { u32::from_be_bytes(bytes[offset..][..4].try_into().unwrap()) }
fn read_u16(bytes: &[u8], offset: usize) -> u16 { u16::from_be_bytes(bytes[offset..][..2].try_into().unwrap()) }
fn read_u8 (bytes: &[u8], offset: usize) -> u8  {  u8::from_be_bytes(bytes[offset..][..1].try_into().unwrap()) }
fn read_i32(bytes: &[u8], offset: usize) -> i32 { i32::from_be_bytes(bytes[offset..][..4].try_into().unwrap()) }
fn read_i8 (bytes: &[u8], offset: usize) -> i8  {  i8::from_be_bytes(bytes[offset..][..1].try_into().unwrap()) }
fn read_array<const SIZE: usize>(bytes: &[u8], offset: usize) -> [u8; SIZE] {
    bytes[offset..][..SIZE].try_into().unwrap()
}

type EventSizes = [u16; 255];

pub fn parse_old_file_slpz(slpz: &[u8]) -> SlpResult<Game> {
    let mut decompressor = slpz::Decompressor::new().ok_or(SlpError::ZstdInitError)?;
    let slp = slpz::decompress(&mut decompressor, slpz)
        .map_err(|_| SlpError::InvalidFile(InvalidLocation::SlpzDecompression))?;
    parse_old_file(&slp)
}

pub fn parse_old_file(slp: &[u8]) -> SlpResult<Game> {
    // parse header and metadata --------------------------------------------------------

    let RawHeaderRet { event_sizes_offset, metadata_offset } = parse_raw_header(slp)?;
    let EventSizesRet { game_start_offset, event_sizes } = event_sizes(slp, event_sizes_offset)?;
    let game_start_size = event_sizes[GAME_START as usize] as usize + 1;
    let game_start = parse_game_start(&slp[game_start_offset..][..game_start_size])?;

    // setup mem for event parsing --------------------------------------------------------

    struct FrameWriteOp {
        pub from_idx: usize,
        pub to: Vec<Frame>,
    }
    let mut frame_ops = [
        FrameWriteOp { from_idx: 0, to: Vec::new() }, FrameWriteOp { from_idx: 0, to: Vec::new() },
        FrameWriteOp { from_idx: 0, to: Vec::new() }, FrameWriteOp { from_idx: 0, to: Vec::new() },
        FrameWriteOp { from_idx: 0, to: Vec::new() }, FrameWriteOp { from_idx: 0, to: Vec::new() },
        FrameWriteOp { from_idx: 0, to: Vec::new() }, FrameWriteOp { from_idx: 0, to: Vec::new() },
    ];
    let mut frame_op_count = 0;

    let frame_count_heuristic = 1024;
    for i in 0..4 {
        if let Some(ch_colour) = game_start.starting_character_colours[i] {
            frame_ops[frame_op_count] = FrameWriteOp {
                from_idx: i,
                to: vec![Frame::NULL; frame_count_heuristic],
            };
            frame_op_count += 1;

            if ch_colour.character() == Character::Popo {
                frame_ops[frame_op_count] = FrameWriteOp {
                    from_idx: i + 4,
                    to: vec![Frame::NULL; frame_count_heuristic],
                };
                frame_op_count += 1;
            }
        }
    }

    let mut pre_frame_temp = [PreFrameUpdate::NULL; 8];
    let mut post_frame_temp = [PostFrameUpdate::NULL; 8];

    // event parsing --------------------------------------------------------

    let mut event_cursor = game_start_offset + game_start_size;
    while event_cursor < metadata_offset {
        let event_cmd = slp[event_cursor];
        let event_size = event_sizes[event_cmd as usize] as usize + 1;
        let event_bytes = &slp[event_cursor..][..event_size];
        event_cursor += event_size;

        match event_cmd {
            PRE_FRAME_UPDATE => {
                let pre_frame = parse_pre_frame_update(event_bytes)?;
                let mut temp_idx = pre_frame.port_idx as usize;
                if pre_frame.is_follower { temp_idx += 4 }
                pre_frame_temp[temp_idx] = pre_frame;
            }
            POST_FRAME_UPDATE => {
                let post_frame = parse_post_frame_update(event_bytes)?;
                let mut temp_idx = post_frame.port_idx as usize;
                if post_frame.is_follower { temp_idx += 4 }
                post_frame_temp[temp_idx] = post_frame;
            }
            FRAME_BOOKEND => {
                let frame_idx = (read_i32(event_bytes, 0x1) + 123) as usize;

                for i in 0..frame_op_count {
                    let op = &mut frame_ops[i];
                    let pre = &pre_frame_temp[op.from_idx];
                    let post = &post_frame_temp[op.from_idx];

                    // no need to special case rollback, just overwrite the frame
                    if op.to.len() <= frame_idx { op.to.resize(frame_idx+1, Frame::NULL); }
                    op.to[frame_idx] = merge_pre_post_frames(pre, post);
                }
            }
            GAME_END => break,
            _ => {}
        }
    }

    // finish up --------------------------------------------------------

    let info = merge_metadata(game_start);

    let mut frames = [None, None, None, None];
    let mut follower_frames = [None, None, None, None];

    for i in 0..frame_op_count {
        let op = &mut frame_ops[i];

        let to = std::mem::replace(&mut op.to, Vec::new());
        let to = Some(to.into_boxed_slice());
        if op.from_idx < 4 {
            frames[op.from_idx] = to;
        } else {
            follower_frames[op.from_idx - 4] = to;
        }
    }

    let frame_count = frames.iter().find(|f| f.is_some()).unwrap().as_ref().unwrap().len();

    let game = Game {
        frame_count,
        frames,
        follower_frames,
        info,
        items: Box::new([]),
        item_idx: Box::new([]),
        stage_info: None,
    };

    Ok(game)
}

// EVENTS ------------------------------------------------------------------------

pub fn parse_game_start(game_start: &[u8]) -> SlpResult<GameStart> {
    if game_start.len() < 5 { return Err(SlpError::InvalidFile(InvalidLocation::GameStart)); }
    if game_start[0] != GAME_START { return Err(SlpError::InvalidFile(InvalidLocation::GameStart)); }

    let version = read_array::<4>(game_start, 1);

    if version[0] < MIN_VERSION_MAJOR { return Err(SlpError::OutdatedFile) }
    if version[0] == MIN_VERSION_MAJOR && version[1] < MIN_VERSION_MINOR { return Err(SlpError::OutdatedFile) }

    let game_info_block = &game_start[5..];

    let stage = Stage::from_u16(read_u16(game_info_block, 0xE))
        .ok_or(SlpError::InvalidFile(InvalidLocation::GameStart))?;

    let timer = read_u32(game_info_block, 0x10);
    
    let mut starting_character_colours = [None; 4];
    for i in 0..4 {
        if read_u8(game_info_block, 0x61 + 0x24*i) == 3 { continue; }

        let character = Character::from_u8_external(read_u8(game_info_block, 0x60 + 0x24*i))
            .ok_or(SlpError::InvalidFile(InvalidLocation::GameStart))?;
        let character_colour = CharacterColour::from_character_and_colour(character, read_u8(game_info_block, 0x63 + 0x24*i))
            .ok_or(SlpError::InvalidFile(InvalidLocation::GameStart))?;

        starting_character_colours[i] = Some(character_colour);
    }

    Ok(GameStart {
        stage,
        starting_character_colours,
        timer,
        names: [[0u8; 31]; 4],
        connect_codes: [[0u8; 10]; 4],
    })
}

pub fn parse_item_update(item_update: &[u8]) -> SlpResult<ItemUpdate> {
    if item_update.len() < 0x2C { return Err(SlpError::InvalidFile(InvalidLocation::ItemUpdate)); }
    if item_update[0] != ITEM_UPDATE { return Err(SlpError::InvalidFile(InvalidLocation::ItemUpdate)); }

    Ok(ItemUpdate {
        frame_idx            : (read_i32(item_update, 0x1) + 123) as u32,
        type_id              : read_u16(item_update, 0x5),
        state                : read_u8(item_update, 0x7),
        direction            : if read_f32(item_update, 0x8) == 1.0 { Direction::Right } else { Direction::Left },
        position             : Vector {
            x                : read_f32(item_update, 0x14),
            y                : read_f32(item_update, 0x18),
        },
        spawn_id             : read_u32(item_update, 0x22),
        missile_type         : read_u8(item_update, 0x26),
        turnip_type          : read_u8(item_update, 0x27),
        charge_shot_launched : read_u8(item_update, 0x28) != 0,
        charge_shot_power    : read_u8(item_update, 0x29),
        owner                : read_i8(item_update, 0x2A),
    })
}

pub type ButtonsMask = u16;
pub mod buttons_mask {
    pub const D_PAD_LEFT  : u16 = 0b0000000000000001;
    pub const D_PAD_RIGHT : u16 = 0b0000000000000010;
    pub const D_PAD_DOWN  : u16 = 0b0000000000000100;
    pub const D_PAD_UP    : u16 = 0b0000000000001000;
    pub const Z           : u16 = 0b0000000000010000;
    pub const R_DIGITAL   : u16 = 0b0000000000100000;
    pub const L_DIGITAL   : u16 = 0b0000000001000000;
    pub const A           : u16 = 0b0000000100000000;
    pub const B           : u16 = 0b0000001000000000;
    pub const X           : u16 = 0b0000010000000000;
    pub const Y           : u16 = 0b0000100000000000;
    pub const START       : u16 = 0b0001000000000000;
}

#[derive(Copy, Clone, Debug)]
struct PreFrameUpdate {
    pub port_idx: u8,
    pub is_follower: bool,
    pub buttons_mask: ButtonsMask,
    pub analog_trigger_value: f32,
    pub left_stick_coords: Vector,
    pub right_stick_coords: Vector,
}

impl PreFrameUpdate {
    const NULL: PreFrameUpdate = PreFrameUpdate {
        port_idx: 0,
        is_follower: false,
        buttons_mask: 0,
        analog_trigger_value: 0.0,
        left_stick_coords: Vector::NULL,
        right_stick_coords: Vector::NULL,
    };
}

fn parse_pre_frame_update(pre_frame_update: &[u8]) -> SlpResult<PreFrameUpdate> {
    Ok(PreFrameUpdate {
        port_idx                      : read_u8(pre_frame_update, 0x5),
        is_follower                   : read_u8(pre_frame_update, 0x6) != 0,
        buttons_mask                  : read_u16(pre_frame_update, 0x31),
        analog_trigger_value          : read_f32(pre_frame_update, 0x29),
        left_stick_coords             : Vector {
            x                         : read_f32(pre_frame_update, 0x19),
            y                         : read_f32(pre_frame_update, 0x1D),
        },
        right_stick_coords            : Vector {
            x                         : read_f32(pre_frame_update, 0x21),
            y                         : read_f32(pre_frame_update, 0x25),
        },
    })
}

#[derive(Copy, Clone, Debug)]
struct PostFrameUpdate {
    pub port_idx: u8,
    pub is_follower: bool,
    pub character: Character,
    pub direction: Direction,
    pub velocity: Vector,
    pub hit_velocity: Vector,
    pub ground_x_velocity: f32,
    pub position: Vector,
    pub state: ActionState,
    pub state_num: u16,
    pub anim_frame: f32,
    pub shield_size: f32,
    pub stock_count: u8,
    pub jumps_remaining: u8,
    pub percent: f32,
    pub is_airborne: bool,
    pub hitlag_frames: f32,
    pub last_ground_idx: u16,
    pub hitstun_misc: f32,
    pub state_flags: [u8; 5],
    pub last_hitting_attack_id: u8,
    pub last_hit_by_instance_id: u16,
}

impl PostFrameUpdate {
    const NULL: PostFrameUpdate = PostFrameUpdate {
        port_idx: 0,
        is_follower: false,
        character: Character::Mario,
        direction: Direction::Left,
        velocity: Vector::NULL,
        hit_velocity: Vector::NULL,
        ground_x_velocity: 0.0,
        position: Vector::NULL,
        state: ActionState::Standard(StandardActionState::DeadDown),
        state_num: 0,
        anim_frame: 0.0,
        shield_size: 0.0,
        stock_count: 0,
        jumps_remaining: 0,
        percent: 0.0,
        is_airborne: false,
        hitlag_frames: 0.0,
        last_ground_idx: 0,
        hitstun_misc: 0.0,
        state_flags: [0u8; 5],
        last_hitting_attack_id: 0,
        last_hit_by_instance_id: 0,
    };
}

fn parse_post_frame_update(post_frame_update: &[u8]) -> SlpResult<PostFrameUpdate> {
    let character = Character::from_u8_internal(post_frame_update[0x7])
        .ok_or(SlpError::InvalidFile(InvalidLocation::PostFrameUpdate))?;

    Ok(PostFrameUpdate {
        port_idx                : read_u8(post_frame_update, 0x5),
        is_follower             : read_u8(post_frame_update, 0x6) != 0,
        character,
        state                   : ActionState::from_u16(read_u16(post_frame_update, 0x8), character)?,
        state_num               : read_u16(post_frame_update, 0x8),
        position                : Vector {
            x                   : read_f32(post_frame_update, 0xA),
            y                   : read_f32(post_frame_update, 0xE),
        },
        direction               : if read_f32(post_frame_update, 0x12) == 1.0 { Direction::Right } else { Direction::Left },
        percent                 : read_f32(post_frame_update, 0x16),
        shield_size             : read_f32(post_frame_update, 0x1A),
        last_hitting_attack_id  : read_u8(post_frame_update, 0x1E),
        stock_count             : read_u8(post_frame_update, 0x21),
        anim_frame              : read_f32(post_frame_update, 0x22),
        ..PostFrameUpdate::NULL
    })
}

fn merge_pre_post_frames(pre: &PreFrameUpdate, post: &PostFrameUpdate) -> Frame {
    Frame {
        character: post.character,
        port_idx: post.port_idx,   
        is_follower: post.is_follower,
        direction: post.direction,    
        velocity: post.velocity,     
        hit_velocity: post.hit_velocity, 
        ground_x_velocity: post.ground_x_velocity, 
        position: post.position,     
        state: post.state,        
        state_num: post.state_num,
        anim_frame: post.anim_frame,   
        shield_size: post.shield_size,
        buttons_mask: pre.buttons_mask,
        analog_trigger_value: pre.analog_trigger_value,
        left_stick_coords: pre.left_stick_coords,
        right_stick_coords: pre.right_stick_coords,
        stock_count: post.stock_count,
        jumps_remaining: post.jumps_remaining,
        is_airborne: post.is_airborne,
        percent: post.percent,
        hitlag_frames: post.hitlag_frames,
        last_ground_idx: post.last_ground_idx,
        hitstun_misc: post.hitstun_misc,
        state_flags: post.state_flags,
        last_hitting_attack_id: post.last_hitting_attack_id,
        last_hit_by_instance_id: post.last_hit_by_instance_id,
    }
}

// HEADER ------------------------------------------------------------------------

#[derive(Copy, Clone, Debug)]
pub struct EventSizesRet {
    pub game_start_offset: usize,
    pub event_sizes: EventSizes,
}

pub fn event_sizes(slp: &[u8], event_sizes_offset: usize) -> SlpResult<EventSizesRet> {
    if slp.len() < event_sizes_offset + 2 { return Err(SlpError::InvalidFile(InvalidLocation::EventSizes)) }
    if slp[event_sizes_offset] != EVENT_PAYLOADS { return Err(SlpError::InvalidFile(InvalidLocation::EventSizes)) }

    let info_size = slp[event_sizes_offset+1] as usize;
    if slp.len() < event_sizes_offset + info_size + 1 { return Err(SlpError::InvalidFile(InvalidLocation::EventSizes)) }
    let event_count = (info_size - 1) / 3;

    let mut event_sizes = [0; 255];
    for i in 0..event_count {
        let offset = event_sizes_offset + 2 + i*3;
        let command_byte = slp[offset] as usize;
        let event_size = read_u16(slp, offset+1);
        event_sizes[command_byte] = event_size;
    }

    Ok(EventSizesRet {
        game_start_offset: event_sizes_offset + info_size + 1,
        event_sizes,
    })
}

// returns offset of metadata
#[derive(Copy, Clone, Debug)]
pub struct RawHeaderRet {
    pub event_sizes_offset: usize,
    pub metadata_offset: usize,
}

pub fn parse_raw_header(slp: &[u8]) -> SlpResult<RawHeaderRet> {
    const HEADER: &'static [u8] = b"{U\x03raw[$U#l";

    if slp.len() < HEADER.len() + 4 { return Err(SlpError::NotAnSlpFile); }

    for i in 0..HEADER.len() {
        if slp[i] != HEADER[i] { return Err(SlpError::NotAnSlpFile) }
    }

    let raw_len = read_u32(slp, HEADER.len()) as usize;
    Ok(RawHeaderRet {
        event_sizes_offset: HEADER.len() + 4,
        metadata_offset: HEADER.len() + raw_len,
    })
}

pub fn parse_file_info(reader: &mut (impl std::io::Read + std::io::Seek)) -> SlpResult<GameStart> {
    let mut buf = [0u8; 1024];
    
    let mut read_count = reader.read(&mut buf)?;

    // unlikely
    while read_count < 1024 {
        let read = reader.read(&mut buf[read_count..])?;
        if read == 0 { break } // file smaller than buffer
        read_count += read;
    }

    let RawHeaderRet { event_sizes_offset, metadata_offset: _ } = parse_raw_header(&buf)?;
    let EventSizesRet { game_start_offset, event_sizes } = event_sizes(&buf, event_sizes_offset)?;
    let game_start_size = event_sizes[GAME_START as usize] as usize + 1;
    let game_start = parse_game_start(&buf[game_start_offset..][..game_start_size])?;

    Ok(game_start)
}

pub fn parse_file_info_slpz(reader: &mut (impl std::io::Read + std::io::Seek)) -> SlpResult<GameStart> {
    let mut buf = [0u8; 4096];
    
    let mut read_count = reader.read(&mut buf)?;

    // unlikely
    while read_count < 24 {
        let read = reader.read(&mut buf[read_count..])?;
        if read == 0 { break } // file smaller than buffer
        read_count += read;
    }

    let version = read_u32(&buf, 0);
    if version > MAX_SUPPORTED_SLPZ_VERSION { return Err(SlpError::TooNewFile) }

    let event_sizes_offset = read_u32(&buf, 4) as usize;
    let game_start_offset = read_u32(&buf, 8) as usize;
    let compressed_events_offset = read_u32(&buf, 16) as usize;

    while read_count < compressed_events_offset && read_count != buf.len() {
        let read = reader.read(&mut buf[read_count..])?;
        if read == 0 { break } // file smaller than buffer
        read_count += read;
    }

    let EventSizesRet { game_start_offset: _, event_sizes } = event_sizes(&buf, event_sizes_offset)?;
    let game_start_size = event_sizes[GAME_START as usize] as usize + 1;
    let game_start = parse_game_start(&buf[game_start_offset..][..game_start_size])?;

    Ok(game_start)
}

fn merge_metadata(game_start: GameStart) -> GameInfo {
    GameInfo {
        stage                      : game_start.stage,
        port_used                  : game_start.starting_character_colours.map(|c| c.is_some()),
        starting_character_colours : game_start.starting_character_colours,
        start_time                 : Time(0),
        timer                      : game_start.timer,
        names: game_start.names,
        connect_codes: game_start.connect_codes,
        duration                   : 0,
    }
}
