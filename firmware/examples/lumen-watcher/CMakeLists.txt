# Lumen — minimal custom firmware for Seeed SenseCAP Watcher
cmake_minimum_required(VERSION 3.16)
set(CMAKE_POLICY_VERSION_MINIMUM 3.5)   # vendored rlottie declares an ancient minimum

include($ENV{IDF_PATH}/tools/cmake/project.cmake)
project(lumen-watcher)
