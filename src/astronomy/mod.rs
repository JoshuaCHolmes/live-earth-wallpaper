//! Astronomical calculations for accurate celestial positioning
//!
//! All calculations are based on the view from Himawari-8's position
//! at 140.7°E longitude in geostationary orbit.

pub mod coordinates;
pub mod moon;
pub mod planets;
pub mod stars;

pub use coordinates::*;
pub use moon::Moon;
pub use planets::PlanetarySystem;
pub use stars::StarCatalog;
