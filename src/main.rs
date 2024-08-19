use std::{
    cell::{OnceCell, RefCell},
    collections::HashMap,
    fs,
    path::PathBuf,
    str::FromStr,
};

use glam::{IVec2, UVec2};
use roast_2d::{ldtk::LdtkProject, prelude::*};

const ACCEL_DEFLATION: f32 = 900.0;
const ACCEL_GROUND: f32 = 600.0;
const ACCEL_AIR: f32 = 300.0;
const PLAYER_JUMP_VEL: f32 = 200.0;
const FRICTION_GROUND: f32 = 2.;
const FRICTION_AIR: f32 = 2.;
const JUMP_HIGH_TIME: f32 = 0.08;
const JUMP_HIGH_ACCEL: f32 = 780.0;
const INFLATION_SPEED: f32 = 1.2;
const MIN_INFLATION: f32 = 1.6;
const MAX_INFLATION: f32 = 8.;
const PLAYER_SIZE: Vec2 = Vec2::new(32.0, 32.0);
const INFLATOR_SPEED: f32 = 0.5;

const SPRITE_SIZE: f32 = 8.0;
const TEXTURE_DIR: &str = "assets/images/";
const DEMO_TEXTURE: &str = "demo.png";
const LEVEL_PATH: &str = "game.ldtk";
const VIEW_SIZE: Vec2 = Vec2::new(512.0, 512.0);
const WINDOW_SIZE: UVec2 = UVec2::new(512, 512);

thread_local! {
    static G: RefCell<Game> = RefCell::new(Game::default());
    static PROJ: RefCell<LdtkProject> = RefCell::new(Default::default());
    static TEXTURE: RefCell<HashMap<String,Image>> = RefCell::new(Default::default());
}

fn load_texture(eng: &mut Engine, filename: &str) -> Image {
    let path = format!("{}/{}", TEXTURE_DIR, filename);
    TEXTURE.with_borrow_mut(|cache| match cache.get(&path) {
        Some(img) => img.clone(),
        None => {
            let img = eng.load_image(&path).expect("load image");
            cache.insert(path, img.clone());
            img
        }
    })
}

fn lerp_size(ori_size: Vec2, inflation_rate: f32) -> Vec2 {
    (ori_size * MAX_INFLATION) * ((inflation_rate) / MAX_INFLATION).powi(2)
}

#[derive(Default)]
pub struct Game {
    pub dead: usize,
    pub current_level: usize,
    pub inflator: f32,
    pub loading_level: Option<usize>,
}

#[repr(u8)]
pub enum Action {
    Left = 1,
    Right,
    Up,
    Down,
    Jump,
    Inflate,
    Deflate,
    Restart,
}

impl From<Action> for ActionId {
    fn from(value: Action) -> Self {
        ActionId(value as u8)
    }
}

#[derive(Default, Clone)]
pub struct Spikes;

impl EntityType for Spikes {
    fn init(&mut self, eng: &mut Engine, ent: &mut Entity) {
        ent.size = Vec2::new(32., 10.);
        let mut sheet = load_texture(eng, DEMO_TEXTURE);
        sheet.scale = ent.size / SPRITE_SIZE;
        sheet.color = RED;
        ent.anim = Some(Animation::new(sheet));
        ent.check_against = EntityGroup::PLAYER;
        ent.physics = EntityPhysics::FIXED;
        ent.gravity = 0.;
    }
    fn touch(&mut self, eng: &mut Engine, ent: &mut Entity, other: &mut Entity) {
        eng.kill(other.ent_ref);
    }
}

#[derive(Default, Clone)]
pub struct Button;

impl EntityType for Button {
    fn init(&mut self, eng: &mut Engine, ent: &mut Entity) {
        ent.size = Vec2::new(32., 32.);
        let mut sheet = load_texture(eng, "hammer.png");
        sheet.scale = ent.size / sheet.sizef();
        // sheet.color = PURPLE;
        ent.anim = Some(Animation::new(sheet));
        ent.check_against = EntityGroup::PLAYER;
        ent.physics = EntityPhysics::FIXED;
        ent.gravity = 0.;
    }
    fn touch(&mut self, eng: &mut Engine, ent: &mut Entity, other: &mut Entity) {
        let mut spikes = Vec::new();
        for ent in eng.world().entities() {
            let Ok(ent) = ent.try_borrow() else {
                continue;
            };
            if ent.ent_type.is::<Spikes>() {
                spikes.push(ent.ent_ref);
            }
        }

        for ent_ref in spikes {
            eng.kill(ent_ref);
        }
    }
}

#[derive(Default, Clone)]
pub struct Inflator;

impl EntityType for Inflator {
    fn init(&mut self, eng: &mut Engine, ent: &mut Entity) {
        let mut sheet = load_texture(eng, "air-pump.png");
        sheet.scale = Vec2::new(32., 32.) / sheet.sizef();
        // sheet.color = BLUE;
        ent.anim = Some(Animation::new(sheet));
        ent.group = EntityGroup::ITEM;
        ent.check_against = EntityGroup::PLAYER;
        ent.physics = EntityPhysics::PASSIVE;
        ent.size = Vec2::new(32., 32.);
        ent.gravity = 0.;
    }
    fn touch(&mut self, eng: &mut Engine, ent: &mut Entity, _other: &mut Entity) {
        // reset inflator
        G.with_borrow_mut(|g| {
            g.inflator = 1.0;
        });
        eng.kill(ent.ent_ref);
    }
}

#[derive(Default, Clone)]
pub struct Door;

impl EntityType for Door {
    fn init(&mut self, eng: &mut Engine, ent: &mut Entity) {
        let mut sheet = load_texture(eng, "exit.png");
        sheet.scale = Vec2::new(32., 32.) / sheet.sizef();
        // sheet.color = Color::rgb(0x5b, 0x6e, 0xe1);
        ent.anim = Some(Animation::new(sheet));
        ent.group = EntityGroup::PICKUP;
        ent.check_against = EntityGroup::PLAYER;
        ent.physics = EntityPhysics::FIXED;
        ent.size = Vec2::new(32., 32.);
        ent.gravity = 0.;
    }
    fn touch(&mut self, eng: &mut Engine, _ent: &mut Entity, _other: &mut Entity) {
        G.with_borrow_mut(|g| {
            g.loading_level = Some(g.current_level + 1);
        });
    }
}

#[derive(Default, Clone)]
pub struct Player {
    can_jump: bool,
    high_jump_time: f32,
    inflation_rate: f32,
    original_size: Vec2,
    normal: Vec2,
    inflation: f32,
}

impl EntityType for Player {
    fn init(&mut self, eng: &mut Engine, ent: &mut Entity) {
        let mut sheet = load_texture(eng, "boogy.png");
        // sheet.color = Color::rgb(0x37, 0x94, 0x6e);
        ent.check_against = EntityGroup::PROJECTILE;
        ent.physics = EntityPhysics::ACTIVE;
        ent.group = EntityGroup::PLAYER;
        ent.gravity = 1.0;
        ent.mass = 1.0;
        self.original_size = PLAYER_SIZE;
        self.inflation_rate = 2.8;
        self.normal.x = 1.;
        ent.size = lerp_size(PLAYER_SIZE, self.inflation_rate).min(PLAYER_SIZE);
        let img_size = sheet.size();
        sheet.scale = ent.size / Vec2::new(img_size.x as f32, img_size.y as f32);
        ent.anim = Some(Animation::new(sheet));

        // init items
        G.with_borrow_mut(|g| {
            g.inflator = 0.0;
        });

        // set camera
        let cam = eng.camera_mut();
        cam.follow(ent.ent_ref, true);
        cam.speed = 3.;
        cam.min_vel = Vec2::splat(5.);
    }

    fn update(&mut self, eng: &mut Engine, ent: &mut Entity) {
        let input = eng.input();

        if input.just_pressed(Action::Restart) {
            eng.kill(ent.ent_ref);
            return;
        }

        ent.accel = Vec2::default();
        ent.friction.x = if ent.on_ground {
            FRICTION_GROUND
        } else {
            FRICTION_AIR
        };

        let inflation;
        if input.pressed(Action::Inflate) && self.inflation_rate < MAX_INFLATION {
            inflation = 1.;
        } else if input.pressed(Action::Deflate) && self.inflation_rate > MIN_INFLATION {
            inflation = -1.;
        } else {
            inflation = 0.;
        }

        // collision detect
        // 1. calc new pos, size...
        // 2. check collision
        // 3. cancel infliction if not possible
        if inflation != 0.0 {
            if inflation > 0.0 {
                let remained = G.with_borrow_mut(|g| {
                    let remained = g.inflator > 0.0;
                    g.inflator = (g.inflator - INFLATOR_SPEED * eng.tick).max(0.0);
                    remained
                });
                if !remained {
                    return;
                }
            }
            let inflation_rate = (self.inflation_rate + inflation * INFLATION_SPEED * eng.tick)
                .clamp(MIN_INFLATION, MAX_INFLATION);
            // let size = self.original_size * inflation_rate;
            let size = lerp_size(self.original_size, inflation_rate);
            // let old_size = self.original_size * self.inflation_rate;
            let old_size = lerp_size(self.original_size, self.inflation_rate);
            let pos = (size - old_size).ceil() * Vec2::new(-0.5, -1.0) + ent.pos;
            // TODO check collition

            let mut collision = false;
            if let Some(map) = eng.collision_map.as_ref() {
                let tile_pos = {
                    let pos = pos / map.tile_size;
                    IVec2::new(pos.x as i32, pos.y as i32)
                };
                let corner_tile_pos = {
                    let pos = (pos + size) / map.tile_size;
                    IVec2::new(pos.x as i32, pos.y as i32)
                };
                'outer: for y in tile_pos.y..=corner_tile_pos.y {
                    for x in tile_pos.x..=corner_tile_pos.x {
                        if !map.get(IVec2::new(x, y)).is_some_and(|v| v == 0) {
                            collision = true;
                            break 'outer;
                        }
                    }
                }
            }

            // do inflation
            if collision {
                return;
            }
            self.inflation_rate = inflation_rate;
            self.inflation = inflation;
            ent.size = size;
            ent.pos = pos;
            ent.mass = (1.0 / self.inflation_rate).clamp(0.1, 1.0);
            ent.gravity = (1.0 / self.inflation_rate).clamp(0.3, 1.0);
            ent.restitution = (self.inflation_rate / 20.0).clamp(0.5, 2.0);
            // Scale sprite image
            if let Some(anim) = ent.anim.as_mut() {
                let img_size = anim.sheet.size();
                anim.sheet.scale = ent.size / Vec2::new(img_size.x as f32, img_size.y as f32);
            }
        } else {
            self.inflation = 0.;
        }

        let mut normal = self.normal;
        if input.pressed(Action::Right) {
            ent.accel.x = if ent.on_ground {
                ACCEL_GROUND
            } else {
                ACCEL_AIR
            };
            self.normal.x = 1.0;
            normal.x = 1.0
        } else if input.pressed(Action::Left) {
            ent.accel.x = -if ent.on_ground {
                ACCEL_GROUND
            } else {
                ACCEL_AIR
            };
            self.normal.x = -1.0;
            normal.x = -1.0
        } else {
            normal.x = 0.0;
        }

        if input.pressed(Action::Up) {
            self.normal.y = -1.0;
            normal.y = -1.0
        } else if input.pressed(Action::Down) {
            self.normal.y = 1.0;
            normal.y = 1.0
        } else {
            self.normal.y = 0.0;
            normal.y = 0.0
        }

        if normal == Vec2::ZERO {
            normal = self.normal;
        }

        if self.inflation < 0. {
            ent.accel += normal * ACCEL_DEFLATION;
        }

        if input.just_pressed(Action::Jump) {
            if ent.on_ground && self.can_jump {
                ent.vel.y = -PLAYER_JUMP_VEL;
                self.can_jump = false;
                self.high_jump_time = JUMP_HIGH_TIME;
            } else if self.high_jump_time > 0. {
                self.high_jump_time -= eng.tick;
                let f = if self.high_jump_time < 0. {
                    eng.tick + self.high_jump_time
                } else {
                    eng.tick
                };
                ent.vel.y -= JUMP_HIGH_ACCEL * f;
            }
        } else {
            self.high_jump_time = 0.;
            self.can_jump = true;
        }

        ent.anim.as_mut().unwrap().sheet.flip_x = (normal.x < 0.);
    }

    fn kill(&mut self, _eng: &mut Engine, _ent: &mut Entity) {
        eprintln!("Player dead... reload level");
        G.with_borrow_mut(|g| {
            g.dead += 1;
            g.loading_level = Some(g.current_level);
        });
    }
}

pub struct Demo {
    frames: f32,
    timer: f32,
    interval: f32,
    font: Option<Font>,
    dead_text: Option<Image>,
    inflator_text: Option<Image>,
}

impl Default for Demo {
    fn default() -> Self {
        Self {
            frames: 0.0,
            timer: 0.0,
            interval: 1.0,
            dead_text: None,
            font: None,
            inflator_text: None,
        }
    }
}

impl Scene for Demo {
    fn init(&mut self, eng: &mut Engine) {
        let view = eng.view_size();

        // bind keys
        let input = eng.input_mut();
        input.bind(KeyCode::Left, Action::Left);
        input.bind(KeyCode::Right, Action::Right);
        input.bind(KeyCode::KeyA, Action::Left);
        input.bind(KeyCode::KeyD, Action::Right);
        input.bind(KeyCode::Up, Action::Up);
        input.bind(KeyCode::KeyW, Action::Up);
        input.bind(KeyCode::Down, Action::Down);
        input.bind(KeyCode::KeyS, Action::Down);
        input.bind(KeyCode::Space, Action::Jump);
        input.bind(KeyCode::KeyI, Action::Inflate);
        input.bind(KeyCode::KeyO, Action::Deflate);
        input.bind(KeyCode::KeyR, Action::Restart);

        // TODO the font path only works on MacOS
        let font_path = "/Library/Fonts/Arial Unicode.ttf";
        if let Ok(font) = Font::open(font_path) {
            self.font.replace(font);
        } else {
            eprintln!("Failed to load font from {font_path}");
        }

        eng.gravity = 400.0;
        let level = G.with_borrow(|g| g.current_level);
        PROJ.with_borrow(|proj| {
            let level = format!("Level_{}", level);
            eng.load_level(proj, &level).unwrap();
        });
    }

    fn update(&mut self, eng: &mut Engine) {
        eng.scene_base_update();
        self.frames += 1.0;
        self.timer += eng.tick;
        if let Some(font) = self.font.clone() {
            let inflator = G.with_borrow(|g| g.inflator);
            let percent = ((inflator * 100.0) as usize).clamp(0, 100);
            let content = format!("Air Pump: {percent}%");
            let text = Text::new(content, font, 30.0, WHITE);
            self.inflator_text = eng.create_text_texture(text).ok();
        }
        if let Some(font) = self.font.clone() {
            let score = G.with_borrow(|g| g.dead);
            let content = format!("Deads: {}", score);
            let text = Text::new(content, font.clone(), 30.0, WHITE);
            self.dead_text = eng.create_text_texture(text).ok();
        }

        if let Some(level) = G.with_borrow_mut(|g| g.loading_level.take()) {
            let level_identifier = format!("Level_{}", level);
            let res = PROJ.with_borrow(|proj| eng.load_level(proj, &level_identifier));
            match res {
                Ok(_) => G.with_borrow_mut(|g| {
                    g.current_level = level;
                    g.inflator = 0.0;
                }),
                Err(err) => {
                    eprintln!("Can't load level {level} err {err:?}");
                }
            }
        }
    }

    fn draw(&mut self, eng: &mut Engine) {
        eng.scene_base_draw();
        if let Some(text) = self.dead_text.as_ref() {
            eng.draw_image(text, Vec2::new(0.0, 0.0));
        }
        if let Some(text) = self.inflator_text.as_ref() {
            let x = eng.view_size().x - text.size().x as f32;
            eng.draw_image(text, Vec2::new(x, 0.0));
        }
    }
}

fn main() {
    G.with_borrow_mut(|g| {
        g.dead = 0;
        g.current_level = 1;
    });
    PROJ.with_borrow_mut(|proj| {
        *proj = {
            let content = fs::read(LEVEL_PATH).unwrap();
            serde_json::from_slice(&content).unwrap()
        };
    });

    let mut eng = Engine::new();
    // set resize and scale
    eng.set_view_size(VIEW_SIZE);
    eng.set_scale_mode(ScaleMode::Exact);
    eng.set_resize_mode(ResizeMode {
        width: true,
        height: true,
    });
    eng.set_sweep_axis(SweepAxis::Y);
    eng.add_entity_type::<Player>();
    eng.add_entity_type::<Door>();
    eng.add_entity_type::<Spikes>();
    eng.add_entity_type::<Button>();
    eng.add_entity_type::<Inflator>();
    eng.set_scene(Demo::default());
    if let Err(err) = run(
        eng,
        "Hello Roast2D".to_string(),
        WINDOW_SIZE.x,
        WINDOW_SIZE.y,
    ) {
        eprintln!("Exit because {err}")
    }
}
