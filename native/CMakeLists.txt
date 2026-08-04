# Worldline native build (Plugin track P.3)
#
# Vendored worldline C++ (from OpenUtau desktop ref) + third_party deps
# (WORLD, libpyin, libgvps, spline, libnpy; xxhash is fetched but unused —
# it was only needed by the trimmed audio_output module).
#
# Produces libworldline.so with the ref-compatible C API:
#   PhraseSynthNew / PhraseSynthDelete / PhraseSynthAddRequest /
#   PhraseSynthSetCurves / PhraseSynthSynth / Resample / F0 / WorldSynthesis ...
#
# Linux:   cmake -B build/build-linux -DCMAKE_BUILD_TYPE=Release && cmake --build build/build-linux
# Android: see build/worldline-android.sh (NDK toolchain, arm64-v8a)

cmake_minimum_required(VERSION 3.16)
project(worldline_native LANGUAGES C CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_C_STANDARD 11)
set(CMAKE_C_STANDARD_REQUIRED ON)
set(CMAKE_POSITION_INDEPENDENT_CODE ON)

if(NOT CMAKE_BUILD_TYPE)
  set(CMAKE_BUILD_TYPE Release)
endif()

set(WORLDLINE_SRC_DIR ${CMAKE_CURRENT_SOURCE_DIR}/plugins/worldline/src)
set(THIRD_PARTY_DIR ${CMAKE_CURRENT_SOURCE_DIR}/third_party)

# NOTE: vendored sources use Bazel-style package includes ("worldline/f0/...").
# The build script creates the self-symlink src/worldline -> . so those
# resolve against WORLDLINE_SRC_DIR without editing any source file.

# ---------------------------------------------------------------------------
# third_party
# ---------------------------------------------------------------------------

# WORLD (mmorise/World @ f8dd5fb + ref patches: Synthesis() gained
# tension/breathiness/voicing args, d4c safe-guard). Compiled as C++.
set(WORLD_SRCS
  ${THIRD_PARTY_DIR}/world/src/cheaptrick.cpp
  ${THIRD_PARTY_DIR}/world/src/codec.cpp
  ${THIRD_PARTY_DIR}/world/src/common.cpp
  ${THIRD_PARTY_DIR}/world/src/d4c.cpp
  ${THIRD_PARTY_DIR}/world/src/dio.cpp
  ${THIRD_PARTY_DIR}/world/src/fft.cpp
  ${THIRD_PARTY_DIR}/world/src/harvest.cpp
  ${THIRD_PARTY_DIR}/world/src/matlabfunctions.cpp
  ${THIRD_PARTY_DIR}/world/src/stonemask.cpp
  ${THIRD_PARTY_DIR}/world/src/synthesis.cpp
  ${THIRD_PARTY_DIR}/world/src/synthesisrealtime.cpp
)
add_library(world STATIC ${WORLD_SRCS})
target_include_directories(world PUBLIC ${THIRD_PARTY_DIR}/world/src)
target_compile_options(world PRIVATE -w)  # vendored C-style code: silence warnings

# WORLD tools/audioio (wavread/wavwrite, used by Resampler's file ctor)
add_library(world_audioio STATIC ${THIRD_PARTY_DIR}/world/tools/audioio.cpp)
target_include_directories(world_audioio PUBLIC ${THIRD_PARTY_DIR}/world/tools)

# libgvps (Sleepwalking/libgvps @ 2f1b410, C, FP_TYPE=double)
add_library(gvps STATIC
  ${THIRD_PARTY_DIR}/libgvps/gvps_full.c
  ${THIRD_PARTY_DIR}/libgvps/gvps_obsrv.c
  ${THIRD_PARTY_DIR}/libgvps/gvps_sampled.c
  ${THIRD_PARTY_DIR}/libgvps/gvps_variable.c
)
target_include_directories(gvps PUBLIC ${THIRD_PARTY_DIR}/libgvps)
target_compile_definitions(gvps PUBLIC FP_TYPE=double)
target_compile_options(gvps PRIVATE -w)

# libpyin (Sleepwalking/libpyin @ b381353 + ref patch: "libgvps/gvps.h" include,
# min/max macro removal). C, FP_TYPE=double. Needs third_party/ on the include
# path so #include "libgvps/gvps.h" resolves.
add_library(pyin STATIC
  ${THIRD_PARTY_DIR}/libpyin/pyin.c
  ${THIRD_PARTY_DIR}/libpyin/yin.c
  ${THIRD_PARTY_DIR}/libpyin/math-funcs.c
)
target_include_directories(pyin PUBLIC
  ${THIRD_PARTY_DIR}/libpyin
  ${THIRD_PARTY_DIR}
)
target_compile_definitions(pyin PUBLIC FP_TYPE=double)
target_compile_options(pyin PRIVATE -w)
target_link_libraries(pyin PUBLIC gvps)

# spline (ttk592/spline @ 5894bea + ref patch) — header-only
add_library(spline INTERFACE)
target_include_directories(spline INTERFACE ${THIRD_PARTY_DIR}/spline/src)

# libnpy (llohse/libnpy v1.0.1) — header-only
add_library(npy INTERFACE)
target_include_directories(npy INTERFACE ${THIRD_PARTY_DIR}/libnpy/include)

# ---------------------------------------------------------------------------
# worldline modules (mirror ref BUILD.bazel structure)
# ---------------------------------------------------------------------------

# classic: classic_args, frq, timing, resampler
add_library(worldline_classic STATIC
  ${WORLDLINE_SRC_DIR}/classic/classic_args.cpp
  ${WORLDLINE_SRC_DIR}/classic/frq.cpp
  ${WORLDLINE_SRC_DIR}/classic/timing.cpp
  ${WORLDLINE_SRC_DIR}/classic/resampler.cpp
)
target_include_directories(worldline_classic PUBLIC
  ${WORLDLINE_SRC_DIR}
  ${THIRD_PARTY_DIR}/absl_compat  # minimal absl::SimpleAtoi/SimpleAtod shim
)
target_link_libraries(worldline_classic
  PUBLIC worldline_common worldline_model worldline_f0 world pyin spline npy
  PRIVATE world_audioio
)

# f0: dio/harvest/pyin/frq estimators
add_library(worldline_f0 STATIC
  ${WORLDLINE_SRC_DIR}/f0/dio_estimator.cpp
  ${WORLDLINE_SRC_DIR}/f0/dio_ss_estimator.cpp
  ${WORLDLINE_SRC_DIR}/f0/frq_estimator.cpp
  ${WORLDLINE_SRC_DIR}/f0/harvest_estimator.cpp
  ${WORLDLINE_SRC_DIR}/f0/pyin_estimator.cpp
)
target_include_directories(worldline_f0 PUBLIC ${WORLDLINE_SRC_DIR})
target_link_libraries(worldline_f0 PUBLIC world pyin worldline_classic)

# model: model (WORLD analysis/synthesis) + effects (spline)
add_library(worldline_model STATIC
  ${WORLDLINE_SRC_DIR}/model/effects.cpp
  ${WORLDLINE_SRC_DIR}/model/model.cpp
)
target_include_directories(worldline_model PUBLIC ${WORLDLINE_SRC_DIR})
target_link_libraries(worldline_model PUBLIC world worldline_common spline)

# common: timer, vec_utils
add_library(worldline_common STATIC
  ${WORLDLINE_SRC_DIR}/common/timer.cpp
  ${WORLDLINE_SRC_DIR}/common/vec_utils.cpp
)
target_include_directories(worldline_common PUBLIC ${WORLDLINE_SRC_DIR})
target_link_libraries(worldline_common PUBLIC npy)

# ---------------------------------------------------------------------------
# Shared library: libworldline.so
# ---------------------------------------------------------------------------
add_library(worldline SHARED
  ${WORLDLINE_SRC_DIR}/worldline.cpp
  ${WORLDLINE_SRC_DIR}/phrase_synth.cpp
  ${WORLDLINE_SRC_DIR}/platinum_stub.cpp
)
target_include_directories(worldline PUBLIC ${WORLDLINE_SRC_DIR})
target_link_libraries(worldline
  PRIVATE
    worldline_classic
    worldline_f0
    worldline_model
    worldline_common
    world
    world_audioio
    pyin
    gvps
    spline
    npy
    m
)
set_target_properties(worldline PROPERTIES OUTPUT_NAME worldline)
