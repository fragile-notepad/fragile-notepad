#include <glib/gstdio.h>
#include <unistd.h>
#include <string.h>
#include "config.h"
#include "selection.h"

static gchar *test_directory;
static gchar *test_file;
static gchar *byte_file;
static gchar *link_file;
static gchar *record_file;
static const gchar *test_editor;
static const gchar *desktop_file;
static const gchar *launcher;

static void
test_cached_selection(void)
{
    FragileSelectionItem items[] = {
        {"/tmp/a space 'quote' $dollar ;semi &amp %.txt", G_FILE_TYPE_REGULAR, TRUE, FALSE},
        {"/tmp/non-UTF8-\xff", G_FILE_TYPE_REGULAR, TRUE, FALSE}
    };
    GError *error = NULL;
    gchar **paths = fragile_selection_from_items(items, G_N_ELEMENTS(items), &error);
    g_assert_no_error(error);
    g_assert_cmpuint(g_strv_length(paths), ==, 2);
    g_assert_cmpstr(paths[0], ==, items[0].path);
    g_assert_cmpstr(paths[1], ==, items[1].path);
    g_assert_true(paths[0] != items[0].path);
    g_strfreev(paths);
}

static void
test_invalid_metadata(void)
{
    FragileSelectionItem invalid[] = {
        {"/tmp/folder", G_FILE_TYPE_DIRECTORY, TRUE, FALSE},
        {"/tmp/virtual", G_FILE_TYPE_REGULAR, FALSE, FALSE},
        {"/tmp/gone", G_FILE_TYPE_REGULAR, TRUE, TRUE},
        {NULL, G_FILE_TYPE_REGULAR, TRUE, FALSE},
        {"relative", G_FILE_TYPE_REGULAR, TRUE, FALSE},
        {"/tmp/unknown", G_FILE_TYPE_UNKNOWN, TRUE, FALSE},
        {"/tmp/fifo", G_FILE_TYPE_SPECIAL, TRUE, FALSE}
    };
    for (gsize i = 0; i < G_N_ELEMENTS(invalid); ++i) {
        FragileSelectionItem mixed[] = {
            {"/tmp/valid", G_FILE_TYPE_REGULAR, TRUE, FALSE}, invalid[i]
        };
        GError *error = NULL;
        g_assert_null(fragile_selection_from_items(mixed, 2, &error));
        g_assert_error(error, G_IO_ERROR, G_IO_ERROR_INVALID_ARGUMENT);
        g_clear_error(&error);
    }
    GError *error = NULL;
    g_assert_null(fragile_selection_from_items(NULL, 0, &error));
    g_assert_error(error, G_IO_ERROR, G_IO_ERROR_INVALID_ARGUMENT);
    g_clear_error(&error);
    g_assert_null(fragile_selection_from_items(NULL, 1, &error));
    g_assert_error(error, G_IO_ERROR, G_IO_ERROR_INVALID_ARGUMENT);
    g_clear_error(&error);
}

static void
test_selection_limit(void)
{
    FragileSelectionItem *items = g_new0(FragileSelectionItem, FRAGILE_SHELL_MAX_FILES);
    for (gsize i = 0; i < FRAGILE_SHELL_MAX_FILES; ++i)
        items[i] = (FragileSelectionItem) {"/tmp/file", G_FILE_TYPE_REGULAR, TRUE, FALSE};
    GError *error = NULL;
    gchar **paths = fragile_selection_from_items(items, FRAGILE_SHELL_MAX_FILES, &error);
    g_assert_no_error(error);
    g_assert_cmpuint(g_strv_length(paths), ==, FRAGILE_SHELL_MAX_FILES);
    gchar **argv = fragile_build_argv("/tmp/editor", (const gchar *const *) paths,
                                    FRAGILE_SHELL_MAX_FILES, &error);
    g_assert_no_error(error);
    g_assert_cmpuint(g_strv_length(argv), ==, FRAGILE_SHELL_MAX_FILES + 2);
    g_strfreev(argv);
    g_strfreev(paths);
    g_assert_null(fragile_selection_from_items(items, FRAGILE_SHELL_MAX_FILES + 1, &error));
    g_assert_error(error, G_IO_ERROR, G_IO_ERROR_INVALID_ARGUMENT);
    g_clear_error(&error);
    g_assert_null(fragile_selection_from_paths(NULL, FRAGILE_SHELL_MAX_FILES + 1, &error));
    g_assert_error(error, G_IO_ERROR, G_IO_ERROR_INVALID_ARGUMENT);
    g_clear_error(&error);
    g_free(items);
}

static void
test_activation_selection(void)
{
    const gchar *paths[] = {test_file, byte_file, link_file};
    GError *error = NULL;
    gchar **valid = fragile_selection_from_paths(paths, G_N_ELEMENTS(paths), &error);
    g_assert_no_error(error);
    for (gsize i = 0; i < G_N_ELEMENTS(paths); ++i)
        g_assert_cmpstr(valid[i], ==, paths[i]);
    g_strfreev(valid);
    const gchar *mixed[] = {test_file, test_directory};
    g_assert_null(fragile_selection_from_paths(mixed, 2, &error));
    g_assert_error(error, G_IO_ERROR, G_IO_ERROR_INVALID_ARGUMENT);
    g_clear_error(&error);
    const gchar *gone[] = {test_file, "/does/not/exist/fragile-shell-test"};
    g_assert_null(fragile_selection_from_paths(gone, 2, &error));
    g_assert_nonnull(error);
    g_clear_error(&error);
}

static void
test_file_url_selection(void)
{
    GFile *native = g_file_new_for_path(test_file);
    GFile *bytes = g_file_new_for_path(byte_file);
    gchar *native_uri = g_file_get_uri(native);
    gchar *byte_uri = g_file_get_uri(bytes);
    g_object_unref(native);
    g_object_unref(bytes);
    const gchar *arguments[] = {native_uri, byte_uri, test_file};
    GError *error = NULL;
    gchar **paths = fragile_selection_from_file_arguments(arguments, 3, &error);
    g_assert_no_error(error);
    g_assert_cmpstr(paths[0], ==, test_file);
    g_assert_cmpstr(paths[1], ==, byte_file);
    g_assert_cmpstr(paths[2], ==, test_file);
    g_strfreev(paths);
    gchar *localhost_uri = g_strconcat("file://localhost", native_uri + strlen("file://"), NULL);
    const gchar *localhost[] = {localhost_uri};
    paths = fragile_selection_from_file_arguments(localhost, 1, &error);
    g_assert_no_error(error);
    g_assert_cmpstr(paths[0], ==, test_file);
    g_strfreev(paths);
    g_free(localhost_uri);
    /* All policy-rejected forms resolve to an existing file if parsed loosely. */
    gchar *policy_invalid[] = {
        g_strconcat("file://remote.example", native_uri + strlen("file://"), NULL),
        g_strconcat("file://user@localhost", native_uri + strlen("file://"), NULL),
        g_strconcat("file://user:password@localhost", native_uri + strlen("file://"), NULL),
        g_strconcat("file://localhost:80", native_uri + strlen("file://"), NULL),
        g_strconcat(native_uri, "?query=1", NULL),
        g_strconcat(native_uri, "#fragment", NULL),
        g_strconcat(native_uri, "%00suffix", NULL),
        g_strconcat("/", test_file, NULL)
    };
    for (gsize i = 0; i < G_N_ELEMENTS(policy_invalid); ++i) {
        const gchar *mixed[] = {native_uri, policy_invalid[i]};
        g_assert_null(fragile_selection_from_file_arguments(mixed, 2, &error));
        g_assert_nonnull(error);
        g_clear_error(&error);
        g_free(policy_invalid[i]);
    }
    const gchar *invalid[] = {
        "smb://server/share/file", "https://example.invalid/file", "trash:///file",
        "file://remote.example/tmp/file", "file://user@localhost/tmp/file",
        "file://user:password@localhost/tmp/file", "file://localhost:80/tmp/file",
        "file:///tmp/file?query=1", "file:///tmp/file#fragment", "file:///tmp/file%00suffix",
        "file:///tmp/invalid%ZZ", "relative-path", NULL
    };
    for (gsize i = 0; i < G_N_ELEMENTS(invalid); ++i) {
        const gchar *mixed[] = {native_uri, invalid[i]};
        g_assert_null(fragile_selection_from_file_arguments(mixed, 2, &error));
        g_assert_nonnull(error);
        g_clear_error(&error);
    }
    g_free(native_uri);
    g_free(byte_uri);
}

static void
test_argument_vector(void)
{
    const gchar *paths[] = {"/tmp/--flag", "/tmp/a\nline ' \" $(`x`) \\ %F \xff"};
    GError *error = NULL;
    gchar **argv = fragile_build_argv("/opt/Fragile Notepad/editor", paths, 2, &error);
    g_assert_no_error(error);
    g_assert_cmpstr(argv[0], ==, "/opt/Fragile Notepad/editor");
    g_assert_cmpstr(argv[1], ==, "--");
    g_assert_cmpstr(argv[2], ==, paths[0]);
    g_assert_cmpstr(argv[3], ==, paths[1]);
    g_assert_null(argv[4]);
    g_strfreev(argv);
    g_assert_null(fragile_build_argv("editor", paths, 2, &error));
    g_assert_error(error, G_IO_ERROR, G_IO_ERROR_INVALID_ARGUMENT);
    g_clear_error(&error);
    const gchar *mixed[] = {paths[0], "relative"};
    g_assert_null(fragile_build_argv("/opt/editor", mixed, 2, &error));
    g_assert_error(error, G_IO_ERROR, G_IO_ERROR_INVALID_ARGUMENT);
    g_clear_error(&error);
}

static void
test_native_launch(void)
{
    gchar *editor_link = g_build_filename(test_directory, "editor with 'quotes' $dollar `ticks` & %.bin", NULL);
    g_assert_cmpint(symlink(test_editor, editor_link), ==, 0);
    const gchar *paths[] = {test_file, byte_file, "/tmp/--file;$(never-run)\nnext"};
    GError *error = NULL;
    GSubprocess *process = fragile_launch(editor_link, paths, 3, &error);
    g_assert_no_error(error);
    g_assert_nonnull(process);
    g_assert_true(g_subprocess_wait_check(process, NULL, &error));
    g_assert_no_error(error);
    g_object_unref(process);
    gchar *record = NULL;
    gsize length = 0;
    g_assert_true(g_file_get_contents(record_file, &record, &length, &error));
    g_assert_no_error(error);
    GString *expected = g_string_new(NULL);
    g_string_append_len(expected, "--", 3);
    for (gsize i = 0; i < G_N_ELEMENTS(paths); ++i)
        g_string_append_len(expected, paths[i], strlen(paths[i]) + 1);
    g_assert_cmpmem(record, length, expected->str, expected->len);
    g_free(record);
    g_string_free(expected, TRUE);
    g_assert_cmpint(g_unlink(editor_link), ==, 0);
    g_free(editor_link);
}

static void
test_launch_failure(void)
{
    const gchar *paths[] = {"/tmp/file"};
    GError *error = NULL;
    g_assert_null(fragile_launch("/does/not/exist/fragile-editor", paths, 1, &error));
    g_assert_nonnull(error);
    g_clear_error(&error);
    gchar *large = g_malloc(256 * 1024 + 1);
    memset(large, 'a', 256 * 1024);
    large[0] = '/';
    large[256 * 1024] = '\0';
    const gchar *oversized[] = {large};
    g_assert_null(fragile_launch(test_editor, oversized, 1, &error));
    g_assert_nonnull(error);
    g_clear_error(&error);
    g_free(large);
}

static void
test_desktop_contract(void)
{
    GKeyFile *desktop = g_key_file_new();
    GError *error = NULL;
    g_assert_true(g_key_file_load_from_file(desktop, desktop_file, G_KEY_FILE_NONE, &error));
    g_assert_no_error(error);
    gchar *exec = g_key_file_get_string(desktop, "Desktop Action FragileNotepadEdit", "Exec", &error);
    g_assert_no_error(error);
    gint argc = 0;
    gchar **argv = NULL;
    g_assert_true(g_shell_parse_argv(exec, &argc, &argv, &error));
    g_assert_no_error(error);
    g_assert_cmpint(argc, ==, 4);
    g_assert_cmpstr(argv[0], ==, FRAGILE_SHELL_LAUNCHER);
    g_assert_cmpstr(argv[1], ==, "--file-urls");
    g_assert_cmpstr(argv[2], ==, "--");
    g_assert_cmpstr(argv[3], ==, "%U");
    g_assert_true(g_str_has_suffix(exec, " --file-urls -- %U"));
    gchar *protocol = g_key_file_get_string(desktop, "Desktop Entry", "X-KDE-Protocol", &error);
    g_assert_no_error(error);
    g_assert_cmpstr(protocol, ==, "file");
    gchar *capabilities = g_key_file_get_string(desktop, "Desktop Entry", "X-KDE-Protocols", &error);
    g_assert_no_error(error);
    g_assert_cmpstr(capabilities, ==, "KIO");
    gchar *mime = g_key_file_get_string(desktop, "Desktop Entry", "MimeType", &error);
    g_assert_no_error(error);
    g_assert_cmpstr(mime, ==, "application/octet-stream;");
    g_assert_cmpint(g_key_file_get_integer(desktop, "Desktop Entry", "X-KDE-MaxNumberOfUrls", &error),
                    ==, FRAGILE_SHELL_MAX_FILES);
    g_assert_no_error(error);
    g_assert_true(g_path_is_absolute(FRAGILE_NOTEPAD_EXECUTABLE));
    g_free(mime);
    g_free(capabilities);
    g_free(protocol);
    g_strfreev(argv);
    g_free(exec);
    g_key_file_unref(desktop);
}

static void
test_launcher_rejects_invalid_selection(void)
{
    const gchar *mixed[] = {launcher, "--", test_file, test_directory, NULL};
    GError *error = NULL;
    GSubprocess *process = g_subprocess_newv(mixed, G_SUBPROCESS_FLAGS_STDERR_PIPE, &error);
    g_assert_no_error(error);
    gchar *stderr_output = NULL;
    g_assert_true(g_subprocess_communicate_utf8(process, NULL, NULL, NULL, &stderr_output, &error));
    g_assert_no_error(error);
    g_assert_cmpint(g_subprocess_get_exit_status(process), ==, 1);
    g_assert_nonnull(strstr(stderr_output, "unable to open selection"));
    g_free(stderr_output);
    g_object_unref(process);
    const gchar *malformed[] = {launcher, test_file, NULL};
    process = g_subprocess_newv(malformed, G_SUBPROCESS_FLAGS_STDERR_PIPE, &error);
    g_assert_no_error(error);
    g_assert_true(g_subprocess_communicate_utf8(process, NULL, NULL, NULL, &stderr_output, &error));
    g_assert_no_error(error);
    g_assert_cmpint(g_subprocess_get_exit_status(process), ==, 2);
    g_assert_nonnull(strstr(stderr_output, "expected [--file-urls] --"));
    g_free(stderr_output);
    g_object_unref(process);
    const gchar *remote[] = {launcher, "--file-urls", "--", test_file, "smb://server/share/file", NULL};
    process = g_subprocess_newv(remote, G_SUBPROCESS_FLAGS_STDERR_PIPE, &error);
    g_assert_no_error(error);
    g_assert_true(g_subprocess_communicate_utf8(process, NULL, NULL, NULL, &stderr_output, &error));
    g_assert_no_error(error);
    g_assert_cmpint(g_subprocess_get_exit_status(process), ==, 1);
    g_assert_nonnull(strstr(stderr_output, "unable to open selection"));
    g_free(stderr_output);
    g_object_unref(process);
}

int
main(int argc, char **argv)
{
    if (argc != 4)
        return 2;
    test_editor = argv[1];
    desktop_file = argv[2];
    launcher = argv[3];
    /* Set the recorder environment before GIO starts any worker threads. */
    GError *error = NULL;
    test_directory = g_dir_make_tmp("fragile-shell-test-XXXXXX", &error);
    g_assert_no_error(error);
    record_file = g_build_filename(test_directory, "record", NULL);
    g_setenv("FRAGILE_SHELL_TEST_OUTPUT", record_file, TRUE);
    test_file = g_build_filename(test_directory, "space ' \" $` & ; %F ? # @ \n.txt", NULL);
    byte_file = g_build_filename(test_directory, "bytes-\xff.txt", NULL);
    link_file = g_build_filename(test_directory, "link.txt", NULL);
    g_assert_true(g_file_set_contents(test_file, "file", -1, &error));
    g_assert_no_error(error);
    g_assert_true(g_file_set_contents(byte_file, "file", -1, &error));
    g_assert_no_error(error);
    g_assert_cmpint(symlink(test_file, link_file), ==, 0);
    argc = 1;
    g_test_init(&argc, &argv, NULL);
    g_test_add_func("/selection/cached-native-paths", test_cached_selection);
    g_test_add_func("/selection/whole-selection-rejection", test_invalid_metadata);
    g_test_add_func("/selection/4096-limit", test_selection_limit);
    g_test_add_func("/selection/activation-file-validation", test_activation_selection);
    g_test_add_func("/selection/file-URL-provenance", test_file_url_selection);
    g_test_add_func("/launch/argument-vector", test_argument_vector);
    g_test_add_func("/launch/native-roundtrip", test_native_launch);
    g_test_add_func("/launch/native-errors", test_launch_failure);
    g_test_add_func("/dolphin/desktop-contract", test_desktop_contract);
    g_test_add_func("/dolphin/native-launcher-invalid-selection", test_launcher_rejects_invalid_selection);
    int result = g_test_run();
    g_unlink(test_file);
    g_unlink(byte_file);
    g_unlink(link_file);
    g_unlink(record_file);
    g_rmdir(test_directory);
    g_free(test_file);
    g_free(byte_file);
    g_free(link_file);
    g_free(record_file);
    g_free(test_directory);
    return result;
}
