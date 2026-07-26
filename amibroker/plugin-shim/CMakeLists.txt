cmake_minimum_required(VERSION 3.20)

# The Rust static library must be built first by scripts/build.ps1.
if(NOT DEFINED RUST_TARGET_TRIPLE)
    if(CMAKE_SIZEOF_VOID_P EQUAL 8)
        set(RUST_TARGET_TRIPLE "x86_64-pc-windows-msvc")
    else()
        set(RUST_TARGET_TRIPLE "i686-pc-windows-msvc")
    endif()
endif()

set(RUST_LIB_DIR "${CMAKE_SOURCE_DIR}/rust-core/target/${RUST_TARGET_TRIPLE}/release")
set(RUST_LIB "${RUST_LIB_DIR}/market_data_core.lib")

add_library(AmiDataPlugin SHARED
    Plugin.cpp
    AmiDate.cpp
    exports.def
)

target_include_directories(AmiDataPlugin PRIVATE
    ${CMAKE_SOURCE_DIR}
    ${CMAKE_SOURCE_DIR}/adk/Include
)

target_compile_definitions(AmiDataPlugin PRIVATE
    _USRDLL
    UNICODE
    _UNICODE
)

target_link_libraries(AmiDataPlugin PRIVATE
    ${RUST_LIB}
    ws2_32
    userenv
    ntdll
    bcrypt
    crypt32
    secur32
    ncrypt
    schannel
    advapi32
    shell32
)

# Match Rust's default dynamically linked MSVC runtime.
set_property(TARGET AmiDataPlugin PROPERTY MSVC_RUNTIME_LIBRARY "MultiThreadedDLL")

set_target_properties(AmiDataPlugin PROPERTIES
    OUTPUT_NAME "AmiDataPlugin"
    SUFFIX ".dll"
)
