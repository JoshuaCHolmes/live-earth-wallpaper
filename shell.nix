{ pkgs ? import <nixpkgs> {} }:

let
  # Windows cross-compilation toolchain
  mingw = pkgs.pkgsCross.mingwW64;
in
pkgs.mkShell {
  nativeBuildInputs = [
    pkgs.cargo
    pkgs.rustc
    pkgs.rust-bindgen
    mingw.stdenv.cc
    mingw.windows.pthreads
  ];

  # Tell cargo where to find the linker
  CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER = "${mingw.stdenv.cc}/bin/x86_64-w64-mingw32-gcc";
  
  # Needed for ring crate (TLS)
  TARGET_CC = "${mingw.stdenv.cc}/bin/x86_64-w64-mingw32-gcc";
  TARGET_AR = "${mingw.stdenv.cc}/bin/x86_64-w64-mingw32-ar";
}
