//! Test handlers shared by the suite and synthetic tests.
#![allow(dead_code)]

use cv_compat::{Actual, Case, Expected, Family, Handler, HandlerError};

/// What a [`Scripted`] handler does with the expected output.
#[derive(Clone, Copy)]
pub enum Mode {
    /// Returns the expected output unchanged.
    Faithful,
    /// Returns the expected output with one change per case.
    Broken,
    /// Panics.
    Panic,
    /// Reports that the feature is not available.
    Pending,
}

pub struct Scripted {
    pub family: Family,
    pub mode: Mode,
}

impl Handler for Scripted {
    fn family(&self) -> Family {
        self.family
    }

    fn run(&self, case: &Case) -> Result<Actual, HandlerError> {
        match (self.mode, &case.expected) {
            (Mode::Panic, _) => panic!("boom in {}", case.id),
            (Mode::Pending, _) => Err(HandlerError::Pending("needs the writer".into())),
            (Mode::Faithful, Expected::Json(value)) => Ok(Actual::Json(value.clone())),
            (Mode::Faithful, Expected::Files(files)) => Ok(Actual::Files(files.clone())),
            (Mode::Broken, Expected::Json(value)) => {
                let mut value = value.clone();
                value["changed"] = serde_json::json!(true);
                Ok(Actual::Json(value))
            }
            (Mode::Broken, Expected::Files(files)) => {
                let mut files = files.clone();
                if let Some(bytes) = files.values_mut().next() {
                    bytes.extend_from_slice(b"x");
                }
                Ok(Actual::Files(files))
            }
        }
    }
}

pub fn all_families(mode: Mode) -> Vec<Box<dyn Handler>> {
    Family::ALL
        .into_iter()
        .map(|family| Box::new(Scripted { family, mode }) as Box<dyn Handler>)
        .collect()
}
