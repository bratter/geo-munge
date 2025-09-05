#[cfg(feature = "debug-print")]
#[macro_export]
macro_rules! debug_println {
    ($condition:expr, $($arg:tt)*) => {
        if $condition {
            eprintln!($($arg)*);
        }
    };
}

#[cfg(not(feature = "debug-print"))]
#[macro_export]
macro_rules! debug_println {
    ($condition:expr, $($arg:tt)*) => {};
}

#[cfg(feature = "debug-print")]
#[macro_export]
macro_rules! debug_print {
    ($condition:expr, $($arg:tt)*) => {
        if $condition {
            eprint!($($arg)*);
        }
    };
}

#[cfg(not(feature = "debug-print"))]
#[macro_export]
macro_rules! debug_print {
    ($condition:expr, $($arg:tt)*) => {};
}

