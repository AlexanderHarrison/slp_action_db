pub const VERSION: u32 = 0;

#[derive(Debug, Clone)]
pub struct Situation {
    pub start_state: slp_parser::BroadState,
    pub action_taken: slp_parser::HighLevelAction,
    pub pos_x: f32,
    pub pos_y: f32,
}

impl Situation {
    pub const WRITTEN_SIZE: usize = 12;
}

#[derive(Debug, Clone)]
pub struct Row {
    pub player_response: Situation,
    pub opponent_initiation: Situation,
    pub score: f32,
}

impl Row {
    pub const WRITTEN_SIZE: usize = Situation::WRITTEN_SIZE * 2 + 4;
}

#[derive(Debug, Clone)]
pub struct Header {
    pub version: u32,
    pub player_character: slp_parser::Character,
    pub opponent_character: slp_parser::Character,
}

impl Header {
    pub const WRITTEN_SIZE: usize = 8;
}

#[derive(Debug, Clone)]
pub enum DBError {
    InvalidFile(&'static str),
    VersionTooNew,
}

pub fn write_header(buf: &mut Vec<u8>, header: &Header) {
    buf.extend_from_slice(&header.version.to_le_bytes());
    buf.push(header.player_character.to_u8_internal());
    buf.push(header.opponent_character.to_u8_internal());
    buf.resize(Header::WRITTEN_SIZE, 0);
}

pub fn write_row(buf: &mut Vec<u8>, row: &Row) {
    buf.extend_from_slice(&row.opponent_initiation.start_state.as_u16().to_le_bytes());
    buf.extend_from_slice(&row.opponent_initiation.action_taken.as_u16().to_le_bytes());
    buf.extend_from_slice(&row.opponent_initiation.pos_x.to_le_bytes());
    buf.extend_from_slice(&row.opponent_initiation.pos_y.to_le_bytes());

    buf.extend_from_slice(&row.player_response.start_state.as_u16().to_le_bytes());
    buf.extend_from_slice(&row.player_response.action_taken.as_u16().to_le_bytes());
    buf.extend_from_slice(&row.player_response.pos_x.to_le_bytes());
    buf.extend_from_slice(&row.player_response.pos_y.to_le_bytes());

    buf.extend_from_slice(&row.score.to_le_bytes());
}

macro_rules! invalid_db { () => { DBError::InvalidFile(concat!(file!(), ":", line!())) } }

pub fn read_header(file: &[u8]) -> Result<Header, DBError> {
    if file.len() < Header::WRITTEN_SIZE { return Err(invalid_db!()); }

    Ok(Header {
        version: read_u32(&file[0..])?,
        player_character: slp_parser::Character::from_u8_internal(read_u8(&file[2..])?)
            .ok_or(invalid_db!())?,
        opponent_character: slp_parser::Character::from_u8_internal(read_u8(&file[3..])?)
            .ok_or(invalid_db!())?,
    })
}

pub fn read_row(file: &[u8], header: &Header) -> Result<Row, DBError> {
    if file.len() < Row::WRITTEN_SIZE { return Err(invalid_db!()); }

    Ok(Row {
        opponent_initiation: Situation {
            start_state: slp_parser::BroadState::from_u16(header.opponent_character, read_u16(&file[0..])?)
                .ok_or(invalid_db!())?,
            action_taken: slp_parser::HighLevelAction::from_u16(header.opponent_character, read_u16(&file[2..])?)
                .ok_or(invalid_db!())?,
            pos_x: read_f32(&file[4..])?,
            pos_y: read_f32(&file[8..])?,
        },
        player_response: Situation {
            start_state: slp_parser::BroadState::from_u16(header.player_character, read_u16(&file[12..])?)
                .ok_or(invalid_db!())?,
            action_taken: slp_parser::HighLevelAction::from_u16(header.player_character, read_u16(&file[14..])?)
                .ok_or(invalid_db!())?,
            pos_x: read_f32(&file[16..])?,
            pos_y: read_f32(&file[20..])?,
        },
        score: read_f32(&file[24..])?,
    })
}

pub fn read_file(file: &[u8]) -> Result<(Header, Vec<Row>), DBError> {
    let header = read_header(file)?;
    if header.version != VERSION { return Err(DBError::VersionTooNew); }

    let mut cursor = Header::WRITTEN_SIZE;
    let row_count = file[cursor..].len() / Row::WRITTEN_SIZE;
    let mut rows = Vec::with_capacity(row_count);

    while cursor < file.len() {
        rows.push(read_row(&file[cursor..], &header)?);
        cursor += Row::WRITTEN_SIZE;
    }

    Ok((header, rows))
}

#[derive(Debug, Clone)]
pub struct SearchSituation {
    pub start_state: slp_parser::BroadState,
    pub pos_x: f32,
    pub pos_y: f32,
}

#[derive(Debug, Clone)]
pub struct SearchQuery {
    pub player_response: SearchSituation,
    pub opponent_initiation: SearchSituation,
}

impl SearchQuery {
    pub fn from_interaction_and_frames(
        interaction: &slp_parser::Interaction,
        player_frames: &[slp_parser::Frame],
        opponent_frames: &[slp_parser::Frame],
    ) -> SearchQuery {
        let pl_frame = &player_frames[interaction.player_response.frame_start];
        let op_frame = &opponent_frames[interaction.opponent_initiation.frame_start];

        SearchQuery {
            player_response: SearchSituation {
                start_state: interaction.player_response.start_state,
                pos_x: pl_frame.position.x,
                pos_y: pl_frame.position.y,
            },
            opponent_initiation: SearchSituation {
                start_state: interaction.opponent_initiation.start_state,
                pos_x: op_frame.position.x,
                pos_y: op_frame.position.y,
            },
        }
    }
}

pub fn search(rows: &[Row], queries: &[SearchQuery]) -> Vec<Vec<Row>> {
    const SEARCH_DISTANCE: f32 = 2.0;
    const SEARCH_DISTANCE_SQ: f32 = SEARCH_DISTANCE*SEARCH_DISTANCE;

    let mut results = vec![Vec::new(); queries.len()];

    for row in rows {
        for (query_i, query) in queries.iter().enumerate() {
            if query.player_response.start_state != row.player_response.start_state { continue; }
            if query.opponent_initiation.start_state != row.opponent_initiation.start_state { continue; }

            let pl_x_dist = query.player_response.pos_x - row.player_response.pos_x;
            let pl_y_dist = query.player_response.pos_y - row.player_response.pos_y;
            let op_x_dist = query.opponent_initiation.pos_x - row.opponent_initiation.pos_x;
            let op_y_dist = query.opponent_initiation.pos_y - row.opponent_initiation.pos_y;
            if pl_x_dist*pl_x_dist + pl_y_dist*pl_y_dist > SEARCH_DISTANCE_SQ { continue; }
            if op_x_dist*op_x_dist + op_y_dist*op_y_dist > SEARCH_DISTANCE_SQ { continue; }

            results[query_i].push(row.clone());
        }
    }

    results
}

fn read_u32(file: &[u8]) -> Result<u32, DBError> {
    if file.len() < 4 { return Err(invalid_db!()); }
    Ok(u32::from_le_bytes(file[..4].try_into().unwrap()))
}

fn read_u16(file: &[u8]) -> Result<u16, DBError> {
    if file.len() < 2 { return Err(invalid_db!()); }
    Ok(u16::from_le_bytes(file[..2].try_into().unwrap()))
}

fn read_u8(file: &[u8]) -> Result<u8, DBError> {
    if file.is_empty() { return Err(invalid_db!()); }
    Ok(file[0])
}

fn read_f32(file: &[u8]) -> Result<f32, DBError> {
    if file.len() < 4 { return Err(invalid_db!()); }
    Ok(f32::from_le_bytes(file[..4].try_into().unwrap()))
}
