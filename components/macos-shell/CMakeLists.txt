cmake_minimum_required(VERSION 3.20)

if(NOT CMAKE_HOST_APPLE)
    message(FATAL_ERROR "The Finder Services component requires macOS and its SDK.")
endif()

if(NOT DEFINED CMAKE_OSX_DEPLOYMENT_TARGET)
    set(CMAKE_OSX_DEPLOYMENT_TARGET "11.0" CACHE STRING "Minimum supported macOS")
endif()
project(FragileNotepadServices VERSION 0.1.0 LANGUAGES C OBJC)

include(cmake/ServiceConfiguration.cmake)
fragile_configure_service_metadata()

find_library(FOUNDATION_FRAMEWORK Foundation REQUIRED)
find_library(APPKIT_FRAMEWORK AppKit REQUIRED)
add_library(fragile_notepad_service_core STATIC src/FragileNotepadService.m)
target_include_directories(fragile_notepad_service_core PUBLIC src)
target_link_libraries(fragile_notepad_service_core PUBLIC
    "${FOUNDATION_FRAMEWORK}" "${APPKIT_FRAMEWORK}")
target_compile_options(fragile_notepad_service_core PRIVATE -fobjc-arc -Wall -Wextra)

add_executable(FragileNotepadServices MACOSX_BUNDLE src/main.m)
target_link_libraries(FragileNotepadServices PRIVATE fragile_notepad_service_core)
target_compile_options(FragileNotepadServices PRIVATE -fobjc-arc -Wall -Wextra)
set_target_properties(FragileNotepadServices PROPERTIES
    OUTPUT_NAME "Fragile Notepad"
    BUNDLE_EXTENSION service
    MACOSX_BUNDLE_INFO_PLIST "${CMAKE_CURRENT_SOURCE_DIR}/Info.plist.in")
install(TARGETS FragileNotepadServices BUNDLE DESTINATION .)
