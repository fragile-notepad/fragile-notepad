# Configure the editor path embedded in the Services bundle.
function(fragile_configure_service_metadata)
    set(FRAGILE_NOTEPAD_EXECUTABLE "" CACHE STRING
        "Absolute path to an installed Fragile Notepad binary or existing portable launcher")
    if(NOT IS_ABSOLUTE "${FRAGILE_NOTEPAD_EXECUTABLE}" OR
       NOT FRAGILE_NOTEPAD_EXECUTABLE MATCHES "^/")
        message(FATAL_ERROR "Set FRAGILE_NOTEPAD_EXECUTABLE to an absolute installation path.")
    endif()

    # Encode XML text once. configure_file substitutes values without evaluating
    # literal variable expressions embedded in the installed executable path.
    set(encoded "${FRAGILE_NOTEPAD_EXECUTABLE}")
    string(REPLACE "&" "&amp;" encoded "${encoded}")
    string(REPLACE "<" "&lt;" encoded "${encoded}")
    string(REPLACE ">" "&gt;" encoded "${encoded}")
    string(REPLACE "\"" "&quot;" encoded "${encoded}")
    string(REPLACE "'" "&apos;" encoded "${encoded}")
    string(REPLACE "\r" "&#13;" encoded "${encoded}")
    set(FRAGILE_NOTEPAD_EXECUTABLE_XML "${encoded}" PARENT_SCOPE)
endfunction()
