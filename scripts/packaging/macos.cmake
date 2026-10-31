set(app "${FRAGILE_REPO_ROOT}/dist/package/Fragile Notepad.app")
foreach(required_file IN ITEMS "Contents/Info.plist" "Contents/MacOS/Fragile Notepad")
    if(NOT EXISTS "${app}/${required_file}")
        message(FATAL_ERROR "Missing ${app}/${required_file}. Run scripts/package-macos.sh dist/package first.")
    endif()
endforeach()

# Preserve the signed app, including its nested runtime and symbolic links.
install(DIRECTORY "${app}" DESTINATION "." USE_SOURCE_PERMISSIONS)
set(CPACK_GENERATOR DragNDrop)
set(CPACK_PACKAGING_INSTALL_PREFIX "/")
set(CPACK_DMG_VOLUME_NAME "Fragile Notepad")
