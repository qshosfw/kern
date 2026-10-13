use kernel::app::{App, AppMetadata, EventStatus, SysContext};
use kernel::event::{Event, KeyCode, KeyState};

pub struct TestFsApp {
    state: u8,
    result_text: &'static str,
}

impl TestFsApp {
    pub const fn new() -> Self {
        Self {
            state: 0,
            result_text: "Press [MENU] to Test FS",
        }
    }
}

const DUMMY_ICON: [u8; 32] = [0u8; 32];

impl<'a> App<crate::services::FirmwareContext<'a>> for TestFsApp {
    fn meta(&self) -> AppMetadata {
        AppMetadata {
            name: "FS Debug",
            icon: &DUMMY_ICON,
        }
    }

    fn on_start(&mut self, _ctx: &mut SysContext) {
        self.state = 0;
        self.result_text = "Press [MENU] to Test FS";
    }

    fn on_event(&mut self, ctx: &mut SysContext, event: Event) -> EventStatus {
        match event {
            Event::Key { code: KeyCode::Menu, state: KeyState::Pressed } => {
                // Write test file
                let test_data = b"KERN_FS_TEST_V1";
                if ctx.storage.write_file("/data/test.bin", test_data).is_err() {
                    self.result_text = "ERR: Write Failed";
                    return EventStatus::Consumed;
                }
                
                // Read back
                let mut buf = [0u8; 32];
                if let Ok(len) = ctx.storage.read_file("/data/test.bin", &mut buf) {
                    if &buf[..len] == test_data {
                        self.result_text = "PASS: R/W OK!";
                    } else {
                        self.result_text = "ERR: Data Corrupt";
                    }
                } else {
                    self.result_text = "ERR: Read Failed";
                }
                
                return EventStatus::Consumed;
            }
            _ => {}
        }
        EventStatus::Ignored
    }

    fn render(&self, canvas: &mut kernel::canvas::Canvas, _ctx: &SysContext) {
        canvas.clear();
        canvas.draw_rect(5, 20, 100, 20, kernel::canvas::Color::Fill);
    }

    fn on_exit(&mut self, _ctx: &mut SysContext) {}
}
