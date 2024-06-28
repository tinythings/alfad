#[cfg(feature = "complex_commands")]
mod complex;
#[cfg(feature = "complex_commands")]
pub use complex::*;

#[cfg(not(feature = "complex_commands"))]
mod simple;
#[cfg(not(feature = "complex_commands"))]
pub use simple::*;
