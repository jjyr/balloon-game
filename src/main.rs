use std::{
    cell::{OnceCell, RefCell},
    fs,
};

use glam::UVec2;
use roast_2d::prelude::*;

const ACCEL_DEFLATION: f32 = 900.0;
const ACCEL_GROUND: f32 = 600.0;
const ACCEL_AIR: f32 = 300.0;
const PLAYER_JUMP_VEL: f32 = 200.0;
const FRICTION_GROUND: f32 = 2.;
const FRICTION_AIR: f32 = 2.;
const JUMP_HIGH_TIME: f32 = 0.08;
const JUMP_HIGH_ACCEL: f32 = 780.0;
const INFLATION_SPEED: f32 = 0.7;

const SPRITE_SIZE: f32 = 8.0;
const BRICK_SIZE: Vec2 = Vec2::new(64., 32.);
const BRICK_DYING: f32 = 0.3;
const TEXTURE_PATH: &str = "assets/images/demo.png";
const LEVEL_PATH: &str = "game.ldtk";
const VIEW_SIZE: Vec2 = Vec2::new(512.0, 512.0);
const WINDOW_SIZE: UVec2 = UVec2::new(512, 512);

thread_local! {
    static G: RefCell<Game> = RefCell::new(Game::default());
    static TEXTURE: OnceCell<Image> = const { OnceCell::new() } ;
}

fn load_texture(eng: &mut Engine) -> Image {
    TEXTURE.with(|t| {
        t.get_or_init(|| eng.load_image(TEXTURE_PATH).unwrap())
            .clone()
    })
}

#[derive(Default)]
pub struct Game {
    pub score: usize,
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
}

impl From<Action> for ActionId {
    fn from(value: Action) -> Self {
        ActionId(value as u8)
    }
}

#[derive(Default, Clone)]
pub struct Brick {
    hit: bool,
    dying: f32,
    dead_pos: Vec2,
}

impl EntityType for Brick {
    fn init(&mut self, eng: &mut Engine, ent: &mut Entity) {
        let mut sheet = load_texture(eng);
        sheet.scale = BRICK_SIZE / SPRITE_SIZE;
        sheet.color = Color::rgb(0x5b, 0x6e, 0xe1);
        ent.anim = Some(Animation::new(sheet));
        ent.size = BRICK_SIZE;
        ent.check_against = EntityGroup::PROJECTILE;
        ent.physics = EntityPhysics::ACTIVE;
    }

    fn kill(&mut self, _eng: &mut Engine, _ent: &mut Entity) {
        G.with_borrow_mut(|g| {
            g.score += 1;
        });
    }

    fn update(&mut self, eng: &mut Engine, ent: &mut Entity) {
        if self.hit {
            self.dying += eng.tick;
            if self.dying > BRICK_DYING {
                eng.kill(ent.ent_ref);
            }

            if let Some(anim) = ent.anim.as_mut() {
                let progress = (self.dying / BRICK_DYING).powi(2);
                let color = {
                    let (r1, g1, b1): (u8, u8, u8) = (0x5b, 0x6e, 0xe1);
                    let (r2, g2, b2) = (RED.r, RED.g, RED.b);
                    let r = r1.saturating_add(((r1 as f32 - r2 as f32) * progress).abs() as u8);
                    let g = g1.saturating_add(((g1 as f32 - g2 as f32) * progress).abs() as u8);
                    let b = b1.saturating_add(((b1 as f32 - b2 as f32) * progress).abs() as u8);
                    Color::rgb(r, g, b)
                };
                let scale = {
                    let start = 1.0;
                    let end = start * 0.5;
                    start - (start - end) * progress
                };
                let size = BRICK_SIZE * scale;
                let center_pos = self.dead_pos + BRICK_SIZE * 0.5;
                ent.pos = center_pos - size * 0.5;
                ent.size = size;

                anim.sheet.scale = size / SPRITE_SIZE;
                anim.sheet.color = color;
            }
        }
    }

    fn touch(&mut self, _eng: &mut Engine, ent: &mut Entity, _other: &mut Entity) {
        if !self.hit {
            self.hit = true;
            self.dead_pos = ent.pos;
        }
    }
}

#[derive(Default, Clone)]
pub struct Player {
    can_jump: bool,
    high_jump_time: f32,
    inflation_rate: f32,
    original_size: Vec2,
    normal: Vec2,
}

impl EntityType for Player {
    fn init(&mut self, eng: &mut Engine, ent: &mut Entity) {
        let mut sheet = load_texture(eng);
        ent.size = Vec2::new(32.0, 32.0);
        sheet.scale = ent.size / SPRITE_SIZE;
        sheet.color = Color::rgb(0x37, 0x94, 0x6e);
        ent.anim = Some(Animation::new(sheet));
        ent.check_against = EntityGroup::PROJECTILE;
        ent.physics = EntityPhysics::ACTIVE;
        ent.group = EntityGroup::PLAYER;
        ent.gravity = 1.0;
        ent.mass = 1.0;
        // ent.restitution = 1.0;
        self.original_size = Vec2::new(32.0, 32.0);
        self.inflation_rate = 1.0;
        self.normal.x = 1.;

        // set camera
        let cam = eng.camera_mut();
        cam.follow(ent.ent_ref, false);
        cam.speed = 3.;
        cam.min_vel = Vec2::splat(5.);
    }

    fn update(&mut self, eng: &mut Engine, ent: &mut Entity) {
        let input = eng.input();

        ent.accel = Vec2::default();
        ent.friction.x = if ent.on_ground {
            FRICTION_GROUND
        } else {
            FRICTION_AIR
        };

        let mut deflation = false;

        if input.pressed(&Action::Inflate.into()) {
            self.inflation_rate += INFLATION_SPEED * eng.tick;
        } else if input.pressed(&Action::Deflate.into()) {
            self.inflation_rate -= INFLATION_SPEED * eng.tick;
            deflation = true;
        }
        ent.size = self.original_size * self.inflation_rate;
        ent.mass = 1.0 / self.inflation_rate;
        ent.gravity = 1.0 / self.inflation_rate;

        let mut normal = self.normal;
        if input.pressed(&Action::Right.into()) {
            ent.accel.x = if ent.on_ground {
                ACCEL_GROUND
            } else {
                ACCEL_AIR
            };
            self.normal.x = 1.0;
            normal.x = 1.0
        } else if input.pressed(&Action::Left.into()) {
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

        if input.pressed(&Action::Up.into()) {
            self.normal.y = -1.0;
            normal.y = -1.0
        } else if input.pressed(&Action::Down.into()) {
            self.normal.y = 1.0;
            normal.y = 1.0
        } else {
            self.normal.y = 0.0;
            normal.y = 0.0
        }

        if normal == Vec2::ZERO {
            normal = self.normal;
        }

        if deflation {
            ent.accel += normal * ACCEL_DEFLATION;
        }

        if input.pressed(&Action::Jump.into()) {
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

        // TODO Check collision
        // TODO push ent if collision
        if let Some(anim) = ent.anim.as_mut() {
            anim.sheet.scale = self.original_size / SPRITE_SIZE * self.inflation_rate;
        }

        eng.entity_base_update(ent);
    }
}

pub struct Demo {
    frames: f32,
    timer: f32,
    interval: f32,
    font: Option<Font>,
    score_text: Option<Image>,
    fps_text: Option<Image>,
}

impl Default for Demo {
    fn default() -> Self {
        Self {
            frames: 0.0,
            timer: 0.0,
            interval: 1.0,
            score_text: None,
            font: None,
            fps_text: None,
        }
    }
}

impl Scene for Demo {
    fn init(&mut self, eng: &mut Engine) {
        let view = eng.view_size();

        // bind keys
        let input = eng.input_mut();
        input.bind(KeyCode::Left, Action::Left.into());
        input.bind(KeyCode::Right, Action::Right.into());
        input.bind(KeyCode::KeyA, Action::Left.into());
        input.bind(KeyCode::KeyD, Action::Right.into());
        input.bind(KeyCode::Up, Action::Up.into());
        input.bind(KeyCode::KeyW, Action::Up.into());
        input.bind(KeyCode::Down, Action::Down.into());
        input.bind(KeyCode::KeyS, Action::Down.into());
        input.bind(KeyCode::Space, Action::Jump.into());
        input.bind(KeyCode::KeyI, Action::Inflate.into());
        input.bind(KeyCode::KeyO, Action::Deflate.into());

        // TODO the font path only works on MacOS
        let font_path = "/Library/Fonts/Arial Unicode.ttf";
        if let Ok(font) = Font::open(font_path) {
            self.font.replace(font);
        } else {
            eprintln!("Failed to load font from {font_path}");
        }

        let proj = {
            let content = fs::read(LEVEL_PATH).unwrap();
            serde_json::from_slice(&content).unwrap()
        };
        eng.gravity = 400.0;
        eng.load_level(&proj, "Level_0").unwrap();
    }

    fn update(&mut self, eng: &mut Engine) {
        eng.scene_base_update();
        self.frames += 1.0;
        self.timer += eng.tick;
        if self.timer > self.interval {
            let fps = self.frames / self.timer;
            self.timer = 0.;
            self.frames = 0.;

            if let Some(font) = self.font.clone() {
                let content = format!("FPS: {:.2}", fps);
                let text = Text::new(content, font, 30.0, WHITE);
                self.fps_text = eng.create_text_texture(text).ok();
            }
        }
        if let Some(font) = self.font.clone() {
            let score = G.with_borrow(|g| g.score);
            let content = format!("Score: {}", score);
            let text = Text::new(content, font.clone(), 30.0, WHITE);
            self.score_text = eng.create_text_texture(text).ok();
        }
    }

    fn draw(&mut self, eng: &mut Engine) {
        eng.scene_base_draw();
        if let Some(text) = self.score_text.as_ref() {
            eng.draw_image(text, Vec2::new(0.0, 0.0));
        }
        if let Some(text) = self.fps_text.as_ref() {
            let x = eng.view_size().x - text.size().x as f32;
            eng.draw_image(text, Vec2::new(x, 0.0));
        }
    }
}

fn main() {
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
    eng.add_entity_type::<Brick>();
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
