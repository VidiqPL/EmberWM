use crate::{native::Native, winit::Nested, CalloopData, Ember};
use std::error::Error;
pub enum Backend {
    Nested(Box<Nested>),
    Native(Box<Native>),
}
impl Backend {
    pub fn process_events(&mut self, data: &mut CalloopData) -> Result<(), Box<dyn Error>> {
        match self {
            Self::Nested(_) => Ok(()),
            Self::Native(b) => b.process_events(data),
        }
    }
    pub fn can_render(&self, state: &Ember) -> bool {
        match self {
            Self::Nested(_) => true,
            Self::Native(b) => b.can_render(state),
        }
    }
    pub fn import_buffers(&mut self, data: &mut CalloopData) {
        match self {
            Self::Nested(b) => b.import_buffers(data),
            Self::Native(b) => b.import_buffers(data),
        }
    }
    pub fn render(&mut self, data: &mut CalloopData) -> Result<(), Box<dyn Error>> {
        match self {
            Self::Nested(b) => b.render(data),
            Self::Native(b) => b.render(data),
        }
    }
}
