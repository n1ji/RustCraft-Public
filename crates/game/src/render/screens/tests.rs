use super::ScreenRenderer;
use crate::render::gui_renderer::GuiRenderer;
use crate::render::scene::ServerListRow;
use crate::render::settings::RenderSettings;
use crate::ui::button_ids as btn;

fn has_button(gui: &GuiRenderer, id: u32) -> bool {
    gui.hit_rect(id).is_some()
}

#[test]
fn settings_screen_keeps_vanilla_button_contract() {
    let mut screen = ScreenRenderer::default();
    let settings = RenderSettings::default();
    let mut gui = GuiRenderer::default();
    screen.rebuild(3, false, &settings, &mut gui);
    for id in [
        btn::VIDEO_SETTINGS,
        btn::CONTROLS,
        btn::LANGUAGE,
        btn::AUDIO_SETTINGS,
        btn::CHAT_SETTINGS,
        btn::SKIN_CUSTOMIZATION,
        btn::RESOURCE_PACKS,
        btn::DONE,
    ] {
        assert!(has_button(&gui, id), "missing options button {id}");
    }
}

#[test]
fn multiplayer_screen_keeps_refresh_and_row_actions() {
    let mut screen = ScreenRenderer::default();
    screen.server_list.push(ServerListRow {
        name: "Test".to_string(),
        address: "127.0.0.1:25565".to_string(),
        ..ServerListRow::default()
    });
    let settings = RenderSettings::default();
    let mut gui = GuiRenderer::default();
    screen.rebuild(4, false, &settings, &mut gui);
    assert!(has_button(&gui, btn::SERVER_ROW_BASE));
    assert!(has_button(&gui, btn::REFRESH_SERVER_LIST));
    assert!(has_button(&gui, btn::MULTIPLAYER_CANCEL));
}

#[test]
fn account_screen_keeps_faces_and_account_actions() {
    let mut screen = ScreenRenderer::default();
    screen.account_list = vec![(
        "Test".to_string(),
        "00000000-0000-0000-0000-000000000000".to_string(),
        true,
    )];
    screen.account_faces.insert(
        "00000000-0000-0000-0000-000000000000".to_string(),
        [[255, 255, 255, 255]; 64],
    );
    let settings = RenderSettings::default();
    let mut gui = GuiRenderer::default();
    screen.rebuild(16, false, &settings, &mut gui);
    assert!(has_button(&gui, btn::ALT_ACCOUNT_ROW_BASE));
    assert!(has_button(&gui, btn::ALT_USE));
    assert!(has_button(&gui, btn::ALT_LOGOUT));
    assert!(has_button(&gui, btn::ALT_BACK));
}
