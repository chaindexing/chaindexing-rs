pub mod states {
    use serde::Deserialize;

    pub const MAX_COUNT: u64 = 1_000;

    #[derive(Clone, Deserialize)]
    pub struct State {
        pub reset_count: u64,
        pub reset_including_side_effects_count: u64,
    }

    impl Default for State {
        fn default() -> Self {
            Self::new()
        }
    }

    impl State {
        pub fn new() -> Self {
            Self {
                reset_count: 0,
                reset_including_side_effects_count: 0,
            }
        }

        pub fn update_reset_count(&mut self, count: u64) {
            self.reset_count = count;
        }
        pub fn update_reset_including_side_effects_count(&mut self, count: u64) {
            self.reset_including_side_effects_count = count;
        }
    }
}

pub use states::State;
