use crate::audio::AudioBackend;
use crate::client;
use crate::client::app::App;
use crate::client::keybind::InputState;
use crate::client::state::GameState;
use crate::client::tick::TickTimer;
use rand::Rng;
use winit::event_loop::ActiveEventLoop;

impl App {
    /// Play the local climbing cadence from vanilla's ladder/vine sound type.
    /// Server movement packets do not carry this client-side movement sound.
    pub(super) fn tick_climbing_sound(&mut self) {
        let x = self.player.position.x.floor() as i32;
        let y = self.player.position.y.floor() as i32;
        let z = self.player.position.z.floor() as i32;
        let block = self.world.get_block(x, y, z);
        let climbing = matches!(
            block,
            crate::world::block::Block::Ladder | crate::world::block::Block::Vine
        );
        if self.ladder_sound_cooldown > 0 {
            self.ladder_sound_cooldown -= 1;
        }
        if climbing && self.player.velocity.y.abs() > 0.01 && self.ladder_sound_cooldown == 0 {
            let sound = block.sound_type();
            self.audio.play(crate::audio::SoundEvent {
                name: sound.step_event().to_string(),
                category: crate::audio::SoundCategory::Players,
                volume: sound.volume() * 0.15,
                pitch: sound.pitch(),
                // Player-local movement sounds use attenuation NONE in vanilla
                // PositionedSoundRecord, so they must remain centred.
                position: None,
            });
            self.ladder_sound_cooldown = 4;
        }
    }
}
