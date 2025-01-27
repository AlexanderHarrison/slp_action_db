mod parse_old_game;

use slp_action_db::*;

fn main() {
    let mut files = std::fs::read_dir("dataset_generator/output/").unwrap()
        .map(|e| e.unwrap().file_name())
        .collect::<Vec<_>>();

    let size = files.len() / 8;
    
    let mut slices = [
        &files[size*0..size*1],
        &files[size*1..size*2],
        &files[size*2..size*3],
        &files[size*3..size*4],
        &files[size*4..size*5],
        &files[size*5..size*6],
        &files[size*6..size*7],
        &files[size*7..],
    ];

    std::thread::scope(|s| {
        let mut handles = [None, None, None, None, None, None, None, None];

        for i in 0..8 {
            let handle = s.spawn(move || {
                let mut rows: Vec<Row> = Vec::with_capacity(8 * 1024 * 1024);
                let thread_files = slices[i];

                for f in thread_files {
                    let path = std::path::Path::new("dataset_generator/output/").join(f);
                    let bytes = std::fs::read(path).unwrap();

                    let game = match parse_old_game::parse_old_file_slpz(&bytes) {
                        Ok(g) => g,
                        Err(e) => {
                            eprintln!("failed to parse: {}", e);
                            continue;
                        }
                    };

                    let Some((low, high)) = game.info.low_high_ports() else {
                        eprintln!("not two player!");
                        continue;
                    };

                    let low_frames = game.frames[low].as_ref().unwrap();
                    let high_frames = game.frames[low].as_ref().unwrap();

                    let low_actions = slp_parser::parse_actions(low_frames);
                    let high_actions = slp_parser::parse_actions(high_frames);

                    let a = slp_parser::generate_interactions(game.info.stage, &low_actions, &high_actions, low_frames, high_frames);
                    let b = slp_parser::generate_interactions(game.info.stage, &high_actions, &low_actions, high_frames, low_frames);

                    rows.reserve(a.len() + b.len());

                    fn push_row(
                        rows: &mut Vec<Row>,
                        interaction: slp_parser::InteractionRef<'_>,
                        pl_frames: &[slp_parser::Frame],
                        op_frames: &[slp_parser::Frame],
                    ) {
                        let Some((s1, s2)) = interaction.score else { return; };

                        let pl_pos = pl_frames[interaction.player_response.frame_start].position;
                        let op_pos = op_frames[interaction.opponent_initiation.frame_start].position;
                    
                        rows.push(Row {
                            opponent_initiation: Situation {
                                start_state: interaction.player_response.start_state,
                                action_taken: interaction.player_response.action_taken,
                                pos_x: pl_pos.x,
                                pos_y: pl_pos.y,
                            },
                            player_response: Situation {
                                start_state: interaction.opponent_initiation.start_state,
                                action_taken: interaction.opponent_initiation.action_taken,
                                pos_x: op_pos.x,
                                pos_y: op_pos.y,
                            },
                            score: (s1.percent + s1.kill + s1.pos_x + s1.pos_y)
                                - (s2.percent + s2.kill + s2.pos_x + s2.pos_y),
                        });
                    }

                    for interaction in a {
                        push_row(&mut rows, interaction, low_frames, high_frames);
                    }

                    for interaction in b {
                        push_row(&mut rows, interaction, high_frames, low_frames);
                    }
                }

                rows
            });

            handles[i] = Some(handle);
        }

        let mut buf: Vec<u8> = Vec::with_capacity(16 * 1024 * 1024);

        write_header(&mut buf, &Header {
            version: VERSION,
            player_character: slp_parser::Character::Fox,
            opponent_character: slp_parser::Character::Fox,
        });

        let mut size = 0usize;
        for i in 0..8 {
            let rows = handles[i].take().unwrap().join().unwrap();

            for row in rows.iter() {
                write_row(&mut buf, row);
            }
        }

        std::fs::write("output.actions", buf).unwrap();
    });
}
