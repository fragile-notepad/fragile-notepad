if(NOT EXISTS "${FRAGILE_BINARY}")
    message(FATAL_ERROR "Missing release executable: ${FRAGILE_BINARY}")
endif()
set(app_icon "${FRAGILE_REPO_ROOT}/target/app-icons/app.ico")
if(NOT EXISTS "${app_icon}")
    message(FATAL_ERROR "Missing application icon. Run scripts/generate_icon_assets.ps1 first.")
endif()

install(PROGRAMS "${FRAGILE_BINARY}" DESTINATION . RENAME fragile-notepad.exe)
fragile_install_notices(.)

set(CPACK_GENERATOR NSIS)
set(CPACK_PACKAGE_INSTALL_DIRECTORY "Fragile Notepad")
set(CPACK_PACKAGE_INSTALL_REGISTRY_KEY "Fragile Notepad")
set(CPACK_PACKAGE_EXECUTABLES fragile-notepad "Fragile Notepad")
set(CPACK_NSIS_INSTALL_ROOT "$PROGRAMFILES64")
set(CPACK_NSIS_EXECUTABLES_DIRECTORY ".")
set(CPACK_NSIS_INSTALLED_ICON_NAME "fragile-notepad.exe")
set(CPACK_NSIS_MUI_ICON "${app_icon}")
set(CPACK_NSIS_MUI_UNIICON "${app_icon}")
set(CPACK_NSIS_URL_INFO_ABOUT "${CPACK_PACKAGE_HOMEPAGE_URL}")
set(CPACK_NSIS_MANIFEST_DPI_AWARE ON)
set(CPACK_NSIS_ENABLE_UNINSTALL_BEFORE_INSTALL ON)
set(CPACK_NSIS_COMPRESSOR "/SOLID lzma")

set(file_icons "${FRAGILE_REPO_ROOT}/target/file-type-icons/windows")
file(READ "${FRAGILE_REPO_ROOT}/assets/icons/file-types/catalog.json" file_types)
string(JSON type_count LENGTH "${file_types}")
math(EXPR last_type "${type_count} - 1")

# Register each type with its own icon. Windows lets the user choose these
# associations through Open With or Default Apps after installation.
set(CPACK_NSIS_EXTRA_INSTALL_COMMANDS [=[
  SetRegView 64
  WriteRegStr HKLM "Software\RegisteredApplications" "Fragile Notepad" "Software\FragileNotepad\Capabilities"
  WriteRegStr HKLM "Software\FragileNotepad\Capabilities" "ApplicationName" "Fragile Notepad"
  WriteRegStr HKLM "Software\FragileNotepad\Capabilities" "ApplicationDescription" "Text editor for notes and source files"
  WriteRegStr HKLM "Software\FragileNotepad\Capabilities" "ApplicationIcon" '$\"$INSTDIR\fragile-notepad.exe$\",0'
]=])
set(CPACK_NSIS_EXTRA_UNINSTALL_COMMANDS [=[
  SetRegView 64
  DeleteRegValue HKLM "Software\RegisteredApplications" "Fragile Notepad"
  DeleteRegKey HKLM "Software\FragileNotepad\Capabilities"
  DeleteRegKey /ifempty HKLM "Software\FragileNotepad"
]=])
foreach(type_index RANGE 0 ${last_type})
    string(JSON type_id GET "${file_types}" ${type_index} id)
    string(JSON type_name GET "${file_types}" ${type_index} name)
    set(type_icon "${file_icons}/${type_id}.ico")
    if(NOT EXISTS "${type_icon}")
        message(FATAL_ERROR "Missing file-type icon: ${type_icon}. Run scripts/generate_file_type_icons.py first.")
    endif()
    install(FILES "${type_icon}" DESTINATION file-type-icons)

    set(type_commands [=[
  WriteRegStr HKLM "Software\Classes\FragileNotepad.@type_id@" "" "@type_name@ file"
  WriteRegStr HKLM "Software\Classes\FragileNotepad.@type_id@\DefaultIcon" "" '$\"$INSTDIR\file-type-icons\@type_id@.ico$\",0'
  WriteRegStr HKLM "Software\Classes\FragileNotepad.@type_id@\shell\open\command" "" '$\"$INSTDIR\fragile-notepad.exe$\" -- $\"%1$\"'
]=])
    string(CONFIGURE "${type_commands}" type_commands @ONLY)
    string(APPEND CPACK_NSIS_EXTRA_INSTALL_COMMANDS "${type_commands}")
    string(APPEND CPACK_NSIS_EXTRA_UNINSTALL_COMMANDS
        "  DeleteRegKey HKLM \"Software\\Classes\\FragileNotepad.${type_id}\"\n")

    string(JSON extension_count LENGTH "${file_types}" ${type_index} extensions)
    if(extension_count GREATER 0)
        math(EXPR last_extension "${extension_count} - 1")
        foreach(extension_index RANGE 0 ${last_extension})
            string(JSON extension GET "${file_types}" ${type_index} extensions ${extension_index})
            set(extension_commands [=[
  WriteRegStr HKLM "Software\Classes\@extension@\OpenWithProgids" "FragileNotepad.@type_id@" ""
  WriteRegStr HKLM "Software\FragileNotepad\Capabilities\FileAssociations" "@extension@" "FragileNotepad.@type_id@"
]=])
            string(CONFIGURE "${extension_commands}" extension_commands @ONLY)
            string(APPEND CPACK_NSIS_EXTRA_INSTALL_COMMANDS "${extension_commands}")
            string(APPEND CPACK_NSIS_EXTRA_UNINSTALL_COMMANDS
                "  DeleteRegValue HKLM \"Software\\Classes\\${extension}\\OpenWithProgids\" \"FragileNotepad.${type_id}\"\n")
        endforeach()
    endif()
endforeach()

# CPack's own installer entries use the default 32-bit registry view.
set(notify_associations [=[
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
  SetRegView 32
]=])
string(APPEND CPACK_NSIS_EXTRA_INSTALL_COMMANDS "${notify_associations}")
string(APPEND CPACK_NSIS_EXTRA_UNINSTALL_COMMANDS "${notify_associations}")
