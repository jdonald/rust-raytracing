fn main() {
    // Windows-specific configuration for shaderc linking
    #[cfg(target_os = "windows")]
    {
        #[cfg(target_env = "msvc")]
        {
            // Configure CMake to use dynamic CRT (/MD) when building shaderc from source
            // This must match Rust's default CRT linkage on Windows
            std::env::set_var("CXXFLAGS", "/MD");
            std::env::set_var("CFLAGS", "/MD");

            // Tell the shaderc-sys build script to use these flags
            std::env::set_var("CMAKE_CXX_FLAGS_RELEASE", "/MD /O2 /Ob2 /DNDEBUG");
            std::env::set_var("CMAKE_C_FLAGS_RELEASE", "/MD /O2 /Ob2 /DNDEBUG");

            // Tell rustc to link against legacy_stdio_definitions on Windows MSVC
            // This provides the missing C runtime symbols like __imp_strncpy, __imp_isdigit, etc.
            println!("cargo:rustc-link-lib=legacy_stdio_definitions");

            // Link against oldnames library which provides some legacy symbols
            println!("cargo:rustc-link-lib=oldnames");

            // Explicitly link against the MSVC C++ standard library
            // This provides symbols like __std_remove_8, __std_search_1
            println!("cargo:rustc-link-lib=msvcprt");
        }
    }
}
