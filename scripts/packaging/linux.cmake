if(NOT EXISTS "${FRAGILE_BINARY}" OR IS_DIRECTORY "${FRAGILE_BINARY}")
    message(FATAL_ERROR "Missing Linux executable: ${FRAGILE_BINARY}. Build it with Cargo or set FRAGILE_BINARY.")
endif()
set(app_icon "${FRAGILE_REPO_ROOT}/target/app-icons/app.png")
if(NOT EXISTS "${app_icon}")
    message(FATAL_ERROR "Missing ${app_icon}; run scripts/generate_icon_assets.sh first.")
endif()

install(PROGRAMS "${FRAGILE_BINARY}" DESTINATION bin RENAME fragile-notepad)
install(FILES "${CMAKE_CURRENT_LIST_DIR}/linux/fragile-notepad.desktop"
        DESTINATION share/applications)
install(FILES "${app_icon}" DESTINATION share/icons/hicolor/1024x1024/apps
        RENAME fragile-notepad.png)
fragile_install_notices("share/doc/fragile-notepad")

set(CPACK_GENERATOR "DEB;RPM")
set(CPACK_PACKAGING_INSTALL_PREFIX "/usr")
set(CPACK_STRIP_FILES OFF)

set(CPACK_DEBIAN_PACKAGE_MAINTAINER "Rachel Fragile, SoundRain")
set(CPACK_DEBIAN_PACKAGE_SECTION editors)
set(CPACK_DEBIAN_PACKAGE_PRIORITY optional)
set(CPACK_DEBIAN_PACKAGE_SHLIBDEPS ON)
set(CPACK_DEBIAN_PACKAGE_DESCRIPTION "${CPACK_PACKAGE_DESCRIPTION}")
# Winit and Softbuffer load windowing libraries dynamically, so ELF scanning
# alone does not find all libraries needed to start the editor.
set(CPACK_DEBIAN_PACKAGE_DEPENDS
    "desktop-file-utils, fontconfig, libx11-6, libx11-xcb1, libxcb1, libxcursor1, libxi6, libxrandr2, libxkbcommon0, libxkbcommon-x11-0, libwayland-client0, libwayland-cursor0, xdg-desktop-portal")
set(CPACK_DEBIAN_PACKAGE_SUGGESTS libvulkan1)

set(CPACK_RPM_PACKAGE_LICENSE "BSD-3-Clause AND Apache-2.0 AND MIT AND OFL-1.1 AND CC-BY-3.0 AND CC-BY-4.0 AND LicenseRef-Fragile-Notepad-Artwork")
set(CPACK_RPM_PACKAGE_GROUP "Applications/Editors")
set(CPACK_RPM_PACKAGE_AUTOREQ ON)
set(CPACK_RPM_PACKAGE_DESCRIPTION "${CPACK_PACKAGE_DESCRIPTION}")
# Cargo already strips the executable. Keep distribution strip/debug scripts
# from modifying the compiled bytes.
set(CPACK_RPM_SPEC_MORE_DEFINE "%define __os_install_post %{nil}")

# Library requirements work with both Fedora and openSUSE package names.
set(library_suffix "")
if(CMAKE_HOST_SYSTEM_PROCESSOR MATCHES "^(x86_64|amd64|aarch64|arm64|ppc64|ppc64le|s390x|riscv64)$")
    set(library_suffix "()(64bit)")
endif()
set(libraries
    libX11.so.6 libX11-xcb.so.1 libxcb.so.1 libXcursor.so.1 libXi.so.6
    libXrandr.so.2 libxkbcommon.so.0 libxkbcommon-x11.so.0
    libwayland-client.so.0 libwayland-cursor.so.0)
set(requires desktop-file-utils fontconfig xdg-desktop-portal)
foreach(library IN LISTS libraries)
    list(APPEND requires "${library}${library_suffix}")
endforeach()
list(JOIN requires ", " CPACK_RPM_PACKAGE_REQUIRES)
set(CPACK_RPM_PACKAGE_SUGGESTS "libvulkan.so.1${library_suffix}")
