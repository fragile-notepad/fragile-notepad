cmake_minimum_required(VERSION 3.20)
project(fragile_notepad_linux_shell VERSION 0.1.0 LANGUAGES C)

if(NOT CMAKE_SYSTEM_NAME STREQUAL "Linux")
    message(FATAL_ERROR "Build the Linux shell component on Linux (including WSL).")
endif()

include(GNUInstallDirs)
include(CTest)
find_package(PkgConfig REQUIRED)
pkg_check_modules(GIO REQUIRED IMPORTED_TARGET gio-2.0>=2.46)

option(FRAGILE_BUILD_NAUTILUS "Build the Nautilus 4 native extension" ON)
set(FRAGILE_NOTEPAD_EXECUTABLE "${CMAKE_INSTALL_FULL_BINDIR}/fragile-notepad"
    CACHE STRING "Absolute installed editor executable; no PATH lookup")
set(FRAGILE_SHELL_LAUNCHER "${CMAKE_INSTALL_FULL_LIBEXECDIR}/fragile-notepad-shell-launcher"
    CACHE STRING "Absolute installed native Dolphin launcher")
set(FRAGILE_DOLPHIN_INSTALL_DIR "${CMAKE_INSTALL_DATADIR}/kio/servicemenus"
    CACHE STRING "Dolphin service menu installation directory")

function(fragile_validate_path variable)
    if(NOT "${${variable}}" MATCHES "^/")
        message(FATAL_ERROR "${variable} must be an absolute Linux path")
    endif()
    if("${${variable}}" MATCHES "[\r\n\t;]")
        message(FATAL_ERROR "${variable} cannot contain tabs, line breaks, or semicolons")
    endif()
    if("${${variable}}" MATCHES "/$")
        message(FATAL_ERROR "${variable} must name an executable, not a directory ending in '/'")
    endif()
endfunction()

fragile_validate_path(FRAGILE_NOTEPAD_EXECUTABLE)
fragile_validate_path(FRAGILE_SHELL_LAUNCHER)
# The Desktop Entry specification forbids '=' in its executable field.
if(FRAGILE_SHELL_LAUNCHER MATCHES "[=%]")
    message(FATAL_ERROR "FRAGILE_SHELL_LAUNCHER cannot contain '=' or '%' in a desktop Exec executable")
endif()
if(FRAGILE_SHELL_LAUNCHER MATCHES "\\\\")
    message(FATAL_ERROR "FRAGILE_SHELL_LAUNCHER cannot contain backslashes in CMake install paths")
endif()

function(fragile_c_string output input)
    string(REPLACE "\\" "\\\\" escaped "${input}")
    string(REPLACE "\"" "\\\"" escaped "${escaped}")
    set(${output} "${escaped}" PARENT_SCOPE)
endfunction()

# Exec quoting is decoded after the .desktop string layer. Escape both layers.
function(fragile_desktop_executable output input)
    string(REPLACE "\\" "\\\\" escaped "${input}")
    string(REPLACE "\"" "\\\"" escaped "${escaped}")
    string(REPLACE "`" "\\`" escaped "${escaped}")
    string(REPLACE "$" "\\$" escaped "${escaped}")
    string(REPLACE "%" "%%" escaped "${escaped}")
    string(REPLACE "\\" "\\\\" escaped "${escaped}")
    set(${output} "\"${escaped}\"" PARENT_SCOPE)
endfunction()

fragile_c_string(FRAGILE_NOTEPAD_EXECUTABLE_C "${FRAGILE_NOTEPAD_EXECUTABLE}")
fragile_c_string(FRAGILE_SHELL_LAUNCHER_C "${FRAGILE_SHELL_LAUNCHER}")
fragile_desktop_executable(FRAGILE_SHELL_LAUNCHER_DESKTOP "${FRAGILE_SHELL_LAUNCHER}")
string(REPLACE "\\" "\\\\" FRAGILE_SHELL_LAUNCHER_TRYEXEC "${FRAGILE_SHELL_LAUNCHER}")
configure_file(src/config.h.in generated/config.h @ONLY)
configure_file(dolphin/fragile-notepad.desktop.in fragile-notepad.desktop @ONLY)

add_library(fragile-shell-common STATIC src/selection.c)
set_target_properties(fragile-shell-common PROPERTIES POSITION_INDEPENDENT_CODE ON C_VISIBILITY_PRESET hidden)
target_compile_features(fragile-shell-common PUBLIC c_std_11)
target_include_directories(fragile-shell-common PUBLIC src "${CMAKE_CURRENT_BINARY_DIR}/generated")
target_link_libraries(fragile-shell-common PUBLIC PkgConfig::GIO)
target_compile_options(fragile-shell-common PRIVATE -Wall -Wextra -Wpedantic)

add_executable(fragile-notepad-shell-launcher src/launcher.c)
target_link_libraries(fragile-notepad-shell-launcher PRIVATE fragile-shell-common)
target_compile_options(fragile-notepad-shell-launcher PRIVATE -Wall -Wextra -Wpedantic)
cmake_path(GET FRAGILE_SHELL_LAUNCHER PARENT_PATH FRAGILE_SHELL_LAUNCHER_DIR)
cmake_path(GET FRAGILE_SHELL_LAUNCHER FILENAME FRAGILE_SHELL_LAUNCHER_NAME)
install(TARGETS fragile-notepad-shell-launcher RUNTIME DESTINATION "${FRAGILE_SHELL_LAUNCHER_DIR}")
# Keep installed filename aligned with the generated desktop entry.
set_target_properties(fragile-notepad-shell-launcher PROPERTIES OUTPUT_NAME "${FRAGILE_SHELL_LAUNCHER_NAME}")
install(FILES "${CMAKE_CURRENT_BINARY_DIR}/fragile-notepad.desktop"
    DESTINATION "${FRAGILE_DOLPHIN_INSTALL_DIR}"
    PERMISSIONS OWNER_READ OWNER_WRITE OWNER_EXECUTE GROUP_READ WORLD_READ)

if(FRAGILE_BUILD_NAUTILUS)
    pkg_check_modules(NAUTILUS REQUIRED IMPORTED_TARGET libnautilus-extension-4)
    pkg_get_variable(NAUTILUS_EXTENSION_DIR libnautilus-extension-4 extensiondir)
    set(FRAGILE_NAUTILUS_INSTALL_DIR "${NAUTILUS_EXTENSION_DIR}"
        CACHE STRING "Nautilus 4 extension directory, defaults to its pkg-config extensiondir")
    if(NOT FRAGILE_NAUTILUS_INSTALL_DIR)
        message(FATAL_ERROR "Set FRAGILE_NAUTILUS_INSTALL_DIR to Nautilus's native extension directory")
    endif()
    add_library(fragile-notepad-nautilus MODULE src/nautilus.c)
    target_link_libraries(fragile-notepad-nautilus PRIVATE fragile-shell-common PkgConfig::NAUTILUS)
    target_compile_options(fragile-notepad-nautilus PRIVATE -Wall -Wextra -Wpedantic)
    set_target_properties(fragile-notepad-nautilus PROPERTIES PREFIX "" C_VISIBILITY_PRESET hidden)
    install(TARGETS fragile-notepad-nautilus LIBRARY DESTINATION "${FRAGILE_NAUTILUS_INSTALL_DIR}")
    if(BUILD_TESTING)
        pkg_check_modules(GMODULE REQUIRED IMPORTED_TARGET gmodule-2.0)
        add_executable(fragile-nautilus-tests tests/test-nautilus.c)
        target_link_libraries(fragile-nautilus-tests PRIVATE PkgConfig::GIO PkgConfig::GMODULE PkgConfig::NAUTILUS)
        target_compile_options(fragile-nautilus-tests PRIVATE -Wall -Wextra -Wpedantic)
        add_test(NAME nautilus-module COMMAND fragile-nautilus-tests "$<TARGET_FILE:fragile-notepad-nautilus>")
    endif()
endif()

if(BUILD_TESTING)
    add_executable(fragile-shell-test-editor tests/test-editor.c)
    target_link_libraries(fragile-shell-test-editor PRIVATE PkgConfig::GIO)
    target_compile_options(fragile-shell-test-editor PRIVATE -Wall -Wextra -Wpedantic)
    add_executable(fragile-shell-tests tests/test-selection.c)
    target_link_libraries(fragile-shell-tests PRIVATE fragile-shell-common)
    target_compile_options(fragile-shell-tests PRIVATE -Wall -Wextra -Wpedantic)
    add_test(NAME linux-shell COMMAND fragile-shell-tests
        "$<TARGET_FILE:fragile-shell-test-editor>"
        "${CMAKE_CURRENT_BINARY_DIR}/fragile-notepad.desktop"
        "$<TARGET_FILE:fragile-notepad-shell-launcher>")
endif()
