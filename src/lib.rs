#[cfg(target_arch = "wasm32")]
pub mod web;
#[cfg(target_arch = "wasm32")]
use web::*;

use std::{cell::RefCell, collections::HashMap, io::Cursor, time::Duration};

use glam::{IVec2, UVec2};
use kira::{
    manager::{AudioManager, AudioManagerSettings, DefaultBackend},
    sound::{
        static_sound::{StaticSoundData, StaticSoundHandle},
        PlaybackState,
    },
    tween::Tween,
};
use rand::{seq::SliceRandom, thread_rng, Rng};
use roast_2d::{handle::Handle, ldtk::LdtkProject, prelude::*};

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

const LEVEL_PATH: &str = "game.ldtk";
const VIEW_SIZE: Vec2 = Vec2::new(512.0, 512.0);
const WINDOW_SIZE: UVec2 = UVec2::new(512, 512);

thread_local! {
    static G: RefCell<Game> = RefCell::new(Game::default());
    static S: RefCell<SoundManager> = RefCell::new(SoundManager::default());
    static PROJ: RefCell<LdtkProject> = RefCell::new(Default::default());
    static TEXTURE: RefCell<HashMap<String, Handle>> = RefCell::new(Default::default());
    static FONT: RefCell<FontManager> = RefCell::new(Default::default());
}

fn load_texture(eng: &mut Engine, path: &str) -> Handle {
    let path = format!("images/{path}");
    TEXTURE.with_borrow_mut(|cache| match cache.get(&path) {
        Some(img) => img.clone(),
        None => {
            let img = eng.assets.load_texture(&path);
            cache.insert(path.to_string(), img.clone());
            img
        }
    })
}

fn lerp_size(ori_size: Vec2, inflation_rate: f32) -> Vec2 {
    (ori_size * MAX_INFLATION) * ((inflation_rate) / MAX_INFLATION).powi(2)
}

pub struct Game {
    pub dead: usize,
    pub current_level: usize,
    pub remained_air: f32,
    pub loading_level: Option<usize>,
}

impl Default for Game {
    fn default() -> Self {
        Self {
            dead: 0,
            current_level: 0,
            remained_air: 0.0,
            loading_level: None,
        }
    }
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

#[derive(Clone)]
pub struct Spikes {
    size: Vec2,
    anim: Animation,
}

impl EntityType for Spikes {
    fn load(eng: &mut Engine) -> Self {
        let size = Vec2::new(32., 10.);
        let texture = load_texture(eng, "spikes.png");
        let sheet = Sprite::with_sizef(texture, size);
        let anim = Animation::new(sheet);
        Self { size, anim }
    }
    fn init(&mut self, _eng: &mut Engine, ent: &mut Entity) {
        ent.size = self.size;
        ent.anim = Some(self.anim.clone());
        ent.check_against = EntityGroup::PLAYER;
        ent.physics = EntityPhysics::FIXED;
        ent.gravity = 0.;
    }
    fn touch(&mut self, eng: &mut Engine, _ent: &mut Entity, other: &mut Entity) {
        eng.kill(other.ent_ref);
    }
}

#[derive(Clone)]
pub struct Crown {
    size: Vec2,
    anim: Animation,
}

impl EntityType for Crown {
    fn load(eng: &mut Engine) -> Self {
        let size = Vec2::new(64., 64.);
        let texture = load_texture(eng, "crown.png");
        let sheet = Sprite::with_sizef(texture, size);
        let anim = Animation::new(sheet);
        Self { size, anim }
    }

    fn init(&mut self, _eng: &mut Engine, ent: &mut Entity) {
        ent.size = self.size;
        ent.anim = Some(self.anim.clone());
        ent.group = EntityGroup::ITEM;
        ent.physics = EntityPhysics::PASSIVE;
        ent.gravity = 0.;
    }
}

#[derive(Clone)]
pub struct Button {
    size: Vec2,
    anim: Animation,
}

impl EntityType for Button {
    fn load(eng: &mut Engine) -> Self {
        let size = Vec2::new(32., 32.);
        let texture = load_texture(eng, "hammer.png");
        let sheet = Sprite::with_sizef(texture, size);
        let anim = Animation::new(sheet);
        Self { size, anim }
    }
    fn init(&mut self, _eng: &mut Engine, ent: &mut Entity) {
        ent.size = self.size;
        ent.anim = Some(self.anim.clone());
        ent.check_against = EntityGroup::PLAYER;
        ent.physics = EntityPhysics::FIXED;
        ent.gravity = 0.;
    }
    fn touch(&mut self, eng: &mut Engine, ent: &mut Entity, _other: &mut Entity) {
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
        eng.kill(ent.ent_ref);
    }
}

#[derive(Clone)]
pub struct Inflator {
    size: Vec2,
    anim: Animation,
}

impl EntityType for Inflator {
    fn load(eng: &mut Engine) -> Self {
        let size = Vec2::new(32., 32.);
        let texture = load_texture(eng, "air-pump.png");
        let sheet = Sprite::with_sizef(texture, size);
        let anim = Animation::new(sheet);
        Self { size, anim }
    }

    fn init(&mut self, _eng: &mut Engine, ent: &mut Entity) {
        ent.size = self.size;
        ent.anim = Some(self.anim.clone());
        ent.group = EntityGroup::ITEM;
        ent.check_against = EntityGroup::PLAYER;
        ent.physics = EntityPhysics::PASSIVE;
        ent.gravity = 0.;
    }
    fn touch(&mut self, eng: &mut Engine, ent: &mut Entity, _other: &mut Entity) {
        // reset inflator
        G.with_borrow_mut(|g| {
            g.remained_air = 1.0;
        });
        eng.kill(ent.ent_ref);
    }
}

#[derive(Clone)]
pub struct Door {
    size: Vec2,
    anim: Animation,
}

impl EntityType for Door {
    fn load(eng: &mut Engine) -> Self {
        let size = Vec2::new(32., 32.);
        let texture = load_texture(eng, "exit.png");
        let sheet = Sprite::with_sizef(texture, size);
        let anim = Animation::new(sheet);
        Self { size, anim }
    }
    fn init(&mut self, _eng: &mut Engine, ent: &mut Entity) {
        ent.size = self.size;
        ent.anim = Some(self.anim.clone());
        ent.group = EntityGroup::PICKUP;
        ent.check_against = EntityGroup::PLAYER;
        ent.physics = EntityPhysics::FIXED;
        ent.gravity = 0.;
    }
    fn touch(&mut self, _eng: &mut Engine, _ent: &mut Entity, _other: &mut Entity) {
        G.with_borrow_mut(|g| {
            g.loading_level = Some(g.current_level + 1);
        });
    }
}

#[derive(Clone)]
pub struct Player {
    can_jump: bool,
    high_jump_time: f32,
    inflation_rate: f32,
    original_size: Vec2,
    normal: Vec2,
    inflation: f32,
    anim: Animation,
    size: Vec2,
}

impl EntityType for Player {
    fn load(eng: &mut Engine) -> Self {
        let original_size = PLAYER_SIZE;
        let inflation_rate = 2.8;
        let normal = Vec2::new(1.0, 0.0);
        let size = lerp_size(PLAYER_SIZE, inflation_rate).min(PLAYER_SIZE);
        let texture = load_texture(eng, "ball.png");
        let sheet = Sprite::with_sizef(texture, size);
        let anim = Animation::new(sheet);

        Self {
            can_jump: false,
            high_jump_time: 0.0,
            inflation_rate,
            original_size,
            normal,
            inflation: 0.0,
            anim,
            size,
        }
    }
    fn init(&mut self, eng: &mut Engine, ent: &mut Entity) {
        ent.check_against = EntityGroup::ITEM;
        ent.physics = EntityPhysics::ACTIVE;
        ent.group = EntityGroup::PLAYER;
        ent.gravity = 1.0;
        ent.mass = 1.0;
        ent.size = self.size;
        ent.anim = Some(self.anim.clone());

        // init items
        G.with_borrow_mut(|g| {
            g.remained_air = 0.0;
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
                    let remained = g.remained_air > 0.0;
                    g.remained_air = (g.remained_air - INFLATOR_SPEED * eng.tick).max(0.0);
                    remained
                });

                if remained {
                    S.with_borrow_mut(|sound| {
                        sound.play_inflate(eng);
                    });
                } else {
                    return;
                }
            }
            let inflation_rate = (self.inflation_rate + inflation * INFLATION_SPEED * eng.tick)
                .clamp(MIN_INFLATION, MAX_INFLATION);
            let size = lerp_size(self.original_size, inflation_rate);
            let old_size = lerp_size(self.original_size, self.inflation_rate);
            let pos = ent.pos + ((size - old_size).ceil() * Vec2::new(0.0, -0.5));

            let mut collision = false;
            if let Some(map) = eng.collision_map.as_ref() {
                let tile_pos = {
                    let pos = ((pos - size * 0.5) / map.tile_size).ceil();
                    IVec2::new(pos.x as i32, pos.y as i32)
                };
                let corner_tile_pos = {
                    let pos = ((pos + size * 0.5) / map.tile_size).floor();
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
            ent.restitution = (self.inflation_rate / 10.0).clamp(0.1, 2.0);
            // Scale sprite image
            if let Some(anim) = ent.anim.as_mut() {
                anim.sheet.size = UVec2::new(size.x as u32, size.y as u32);
            }
        } else {
            self.inflation = 0.;
            S.with_borrow_mut(|sound| {
                if let Some(mut s) = sound.playing.take() {
                    s.stop(Tween {
                        duration: Duration::from_secs_f32(0.5),
                        ..Default::default()
                    })
                }
            });
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

            S.with_borrow_mut(|sound| {
                sound.play_deflate(eng);
            });
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
            self.can_jump = ent.on_ground;
        }

        ent.anim.as_mut().unwrap().sheet.flip_x = normal.x < 0.;
    }

    fn collide(
        &mut self,
        eng: &mut Engine,
        ent: &mut Entity,
        _normal: Vec2,
        _trace: Option<&Trace>,
    ) {
        if !self.can_jump && (ent.vel.x.abs() + ent.vel.y.abs()) > 120.0 {
            S.with_borrow_mut(|sound| {
                sound.play_collide(eng);
            });
        }
    }

    fn touch(&mut self, eng: &mut Engine, ent: &mut Entity, other: &mut Entity) {
        if other.ent_type.is::<Crown>() {
            eng.kill(other.ent_ref);

            self.original_size *= 2.0;
            let size = lerp_size(self.original_size, self.inflation_rate).min(self.original_size);
            ent.size = size;
            let texture = load_texture(eng, "ball-king.png");
            let sheet = Sprite::with_sizef(texture, size);
            ent.anim = Some(Animation::new(sheet));
        }
    }

    fn kill(&mut self, eng: &mut Engine, _ent: &mut Entity) {
        eprintln!("Player dead... reload level");
        G.with_borrow_mut(|g| {
            g.dead += 1;
            g.loading_level = Some(g.current_level);
        });
        S.with_borrow_mut(|sound| sound.play_killed(eng));
    }
}

pub struct Loading {
    handle: Handle,
}

impl Scene for Loading {
    fn update(&mut self, eng: &mut Engine) {
        log::info!("Loading....");
        if let Some(data) = eng.assets.get_raw(&self.handle) {
            PROJ.with_borrow_mut(|proj| {
                *proj = serde_json::from_slice(data).unwrap();
            });

            eng.set_scene(Demo::default());
        }
    }
}

pub struct Demo {
    frames: f32,
    timer: f32,
    dead_text: Option<Sprite>,
    remained_air_text: Option<Sprite>,
}

impl Default for Demo {
    fn default() -> Self {
        Self {
            frames: 0.0,
            timer: 0.0,
            dead_text: None,
            remained_air_text: None,
        }
    }
}

impl Scene for Demo {
    fn init(&mut self, eng: &mut Engine) {
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

        eng.gravity = 400.0;
        let level = G.with_borrow(|g| g.current_level);
        PROJ.with_borrow(|proj| {
            let level = format!("Level_{}", level);
            eng.load_level(proj, &level).unwrap();
            log::info!("Here we go.... {level}");
        });
    }

    fn update(&mut self, eng: &mut Engine) {
        eng.scene_base_update();
        self.frames += 1.0;
        self.timer += eng.tick;

        // render text
        FONT.with_borrow_mut(|font| {
            if let Some(font) = font.fetch(eng) {
                self.remained_air_text.replace({
                    let percent =
                        ((G.with_borrow(|g| g.remained_air) * 100.0) as usize).clamp(0, 100);
                    let content = format!("{percent}%");
                    let text = Text::new(content, font.clone(), 28.0, Color::rgb(0x42, 0xbf, 0xe8));
                    let (texture, size) = eng.create_text_texture(text);
                    Sprite::new(texture, size)
                });
                self.dead_text.replace({
                    let content = format!("{}", G.with_borrow(|g| g.dead));
                    let text = Text::new(content, font, 28.0, GRAY);
                    let (texture, size) = eng.create_text_texture(text);
                    Sprite::new(texture, size)
                });
            }
        });

        if let Some(level) = G.with_borrow_mut(|g| g.loading_level.take()) {
            let level_identifier = format!("Level_{}", level);
            let res = PROJ.with_borrow(|proj| eng.load_level(proj, &level_identifier));
            match res {
                Ok(_) => G.with_borrow_mut(|g| {
                    g.current_level = level;
                    g.remained_air = 0.0;
                }),
                Err(err) => {
                    eprintln!("Can't load level {level} err {err:?}");
                }
            }
        }
    }

    fn draw(&mut self, eng: &mut Engine) {
        eng.scene_base_draw();
        let mut y_offset = 0.0;
        if let Some(text) = self.dead_text.as_ref() {
            let texture = load_texture(eng, "ball-death.png");
            let death = Sprite::with_sizef(texture, Vec2::new(28.0, 24.0));
            eng.draw_image(&death, death.sizef() / 2.0, None, None);
            y_offset += -death.sizef().y;
            eng.draw_image(
                text,
                Vec2::new(death.sizef().x * 0.5, y_offset) + text.sizef() / 2.0,
                None,
                None,
            );
            y_offset += text.sizef().y;
        }
        if let Some(text) = self.remained_air_text.as_ref() {
            let texture = load_texture(eng, "air-pump.png");
            let air_pump = Sprite::new(texture, UVec2::splat(32));
            eng.draw_image(
                &air_pump,
                Vec2::new(0.0, y_offset) + air_pump.sizef() / 2.0,
                None,
                None,
            );
            y_offset += -air_pump.sizef().y * 0.5;
            eng.draw_image(
                text,
                Vec2::new(air_pump.sizef().x * 0.5, y_offset) + text.sizef() / 2.0,
                None,
                None,
            );
        }
    }
}

#[derive(Debug)]
pub enum SoundType {
    Jump,
    Inflate,
    Death,
}

pub struct SoundManager {
    audio: AudioManager<DefaultBackend>,
    sounds_data: HashMap<Handle, StaticSoundData>,
    jumps: Vec<Handle>,
    inflate: Option<Handle>,
    death: Option<Handle>,
    playing: Option<StaticSoundHandle>,
}

impl Default for SoundManager {
    fn default() -> Self {
        let audio = AudioManager::<DefaultBackend>::new(AudioManagerSettings::default()).unwrap();
        Self {
            audio,
            sounds_data: Default::default(),
            jumps: Default::default(),
            inflate: None,
            death: None,
            playing: None,
        }
    }
}

impl SoundManager {
    fn load(&mut self, eng: &mut Engine) {
        self.jumps = (1..=8)
            .map(|i| {
                eng.assets
                    .load_bytes(format!("sounds/arrowHit/arrowHit0{i}.wav"))
            })
            .collect();
        self.inflate
            .replace(eng.assets.load_bytes("sounds/48_Speed_up_02.wav"));
        self.death
            .replace(eng.assets.load_bytes("sounds/21_Debuff_01.wav"));
    }

    fn fetch(&mut self, eng: &Engine, sound: SoundType) -> Option<StaticSoundData> {
        let handle = match sound {
            SoundType::Jump => {
                let mut rng = thread_rng();
                self.jumps.choose(&mut rng)?
            }
            SoundType::Inflate => self.inflate.as_ref()?,
            SoundType::Death => self.death.as_ref()?,
        };
        match self.sounds_data.get(handle) {
            Some(data) => {
                log::debug!("Get sound {sound:?} cached");
                Some(data.to_owned())
            }
            None => {
                let Some(raw) = eng.assets.get_raw(handle).cloned() else {
                    log::debug!("Get sound {sound:?} not ready");
                    return None;
                };
                log::debug!("Get sound {sound:?} done");
                let data = StaticSoundData::from_media_source(Cursor::new(raw)).unwrap();
                self.sounds_data.insert(handle.to_owned(), data.clone());
                Some(data)
            }
        }
    }

    fn play_collide(&mut self, eng: &Engine) {
        let Some(s) = self.fetch(eng, SoundType::Jump) else {
            return;
        };
        let mut s = self.audio.play(s).unwrap();
        s.set_volume(0.3, Default::default());
        let mut rng = thread_rng();
        let rate = rng.gen_range(2.8..3.4);
        s.set_playback_rate(rate, Tween::default());
    }

    fn play_inflate(&mut self, eng: &Engine) {
        if self
            .playing
            .as_ref()
            .is_some_and(|s| s.state() == PlaybackState::Playing)
        {
            return;
        };
        if let Some(s) = self.fetch(eng, SoundType::Inflate) {
            let mut s = self.audio.play(s).unwrap();
            s.set_loop_region(0.0..1.0);
            s.set_volume(0.5, Default::default());
            s.set_playback_rate(2.4, Tween::default());
            self.playing.replace(s);
        }
    }

    fn play_deflate(&mut self, eng: &Engine) {
        if self
            .playing
            .as_ref()
            .map(|s| s.state() == PlaybackState::Playing && s.position() < 2.0)
            .unwrap_or_default()
        {
            return;
        };
        let Some(s) = self.fetch(eng, SoundType::Inflate).clone() else {
            return;
        };
        let mut s = self.audio.play(s).unwrap();
        s.set_volume(0.5, Default::default());
        s.set_playback_rate(3.8, Tween::default());
        self.playing.replace(s);
    }

    fn play_killed(&mut self, eng: &Engine) {
        if let Some(s) = self.fetch(eng, SoundType::Death) {
            let mut sound = self.audio.play(s).unwrap();
            sound.set_playback_rate(2., Tween::default());
        }
    }
}

#[derive(Default)]
pub struct FontManager {
    handle: Option<Handle>,
    font: Option<Font>,
}
impl FontManager {
    fn load(&mut self, eng: &mut Engine) {
        self.handle
            .replace(eng.assets.load_bytes("fonts/OpenSans-Bold.ttf"));
    }

    fn fetch(&mut self, eng: &mut Engine) -> Option<Font> {
        match self.font.clone() {
            Some(font) => Some(font),
            None => {
                let handle = self.handle.as_ref()?;
                let raw = eng.assets.get_raw(handle)?;
                let font = Font::from_bytes(raw.to_owned());
                self.font = font.clone();
                font
            }
        }
    }
}

pub fn app() -> App {
    App::default()
        .title("Balloon Game".to_string())
        .window(WINDOW_SIZE)
        .vsync(true)
}

pub fn setup(eng: &mut Engine) {
    // Setup game state
    G.with_borrow_mut(|g| {
        g.dead = 0;
        g.current_level = 0;
    });

    // Load LDTK project
    let handle = eng.assets.load_bytes(LEVEL_PATH);

    // load sounds
    S.with_borrow_mut(|s| {
        s.load(eng);
    });

    FONT.with_borrow_mut(|font| {
        font.load(eng);
    });

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
    eng.add_entity_type::<Crown>();
    eng.set_scene(Loading { handle });
}
