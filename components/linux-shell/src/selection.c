#include "selection.h"
#include <string.h>

static gboolean
validate_count(gsize count, GError **error)
{
    if (count == 0 || count > FRAGILE_SHELL_MAX_FILES) {
        g_set_error(error, G_IO_ERROR, G_IO_ERROR_INVALID_ARGUMENT,
                    "Select between 1 and %u local files", FRAGILE_SHELL_MAX_FILES);
        return FALSE;
    }
    return TRUE;
}

static gboolean
validate_path(const gchar *path, GError **error)
{
    if (path == NULL || !g_path_is_absolute(path)) {
        g_set_error_literal(error, G_IO_ERROR, G_IO_ERROR_INVALID_ARGUMENT,
                            "Every selected file must have an absolute native path");
        return FALSE;
    }
    return TRUE;
}

gchar **
fragile_selection_from_items(const FragileSelectionItem *items,
                            gsize count, GError **error)
{
    if (!validate_count(count, error))
        return NULL;
    if (items == NULL) {
        g_set_error_literal(error, G_IO_ERROR, G_IO_ERROR_INVALID_ARGUMENT,
                            "Selection metadata is missing");
        return NULL;
    }
    gchar **paths = g_new0(gchar *, count + 1);
    for (gsize i = 0; i < count; ++i) {
        if (!items[i].native || items[i].gone || items[i].type != G_FILE_TYPE_REGULAR) {
            g_set_error_literal(error, G_IO_ERROR, G_IO_ERROR_INVALID_ARGUMENT,
                                "Every selected item must be an existing local regular file");
            g_strfreev(paths);
            return NULL;
        }
        if (!validate_path(items[i].path, error)) {
            g_strfreev(paths);
            return NULL;
        }
        paths[i] = g_strdup(items[i].path);
    }
    return paths;
}

gchar **
fragile_selection_from_paths(const gchar *const *paths, gsize count, GError **error)
{
    if (!validate_count(count, error))
        return NULL;
    if (paths == NULL) {
        g_set_error_literal(error, G_IO_ERROR, G_IO_ERROR_INVALID_ARGUMENT,
                            "Selection paths are missing");
        return NULL;
    }
    gchar **result = g_new0(gchar *, count + 1);
    for (gsize i = 0; i < count; ++i) {
        if (!validate_path(paths[i], error)) {
            g_strfreev(result);
            return NULL;
        }
        GFile *file = g_file_new_for_path(paths[i]);
        GFileInfo *info = g_file_query_info(file, G_FILE_ATTRIBUTE_STANDARD_TYPE,
                                          G_FILE_QUERY_INFO_NONE, NULL, error);
        g_object_unref(file);
        if (info == NULL) {
            g_strfreev(result);
            return NULL;
        }
        gboolean regular = g_file_info_get_file_type(info) == G_FILE_TYPE_REGULAR;
        g_object_unref(info);
        if (!regular) {
            g_set_error_literal(error, G_IO_ERROR, G_IO_ERROR_INVALID_ARGUMENT,
                                "Every selected item must be an existing local regular file");
            g_strfreev(result);
            return NULL;
        }
        result[i] = g_strdup(paths[i]);
    }
    return result;
}

gchar **
fragile_selection_from_file_arguments(const gchar *const *arguments,
                                     gsize count, GError **error)
{
    if (!validate_count(count, error))
        return NULL;
    if (arguments == NULL) {
        g_set_error_literal(error, G_IO_ERROR, G_IO_ERROR_INVALID_ARGUMENT,
                            "Selection arguments are missing");
        return NULL;
    }
    gchar **paths = g_new0(gchar *, count + 1);
    for (gsize i = 0; i < count; ++i) {
        const gchar *argument = arguments[i];
        if (argument != NULL && g_path_is_absolute(argument)) {
            /* Qt can convert file://host/path to //host/path in its %U expansion. */
            if (g_str_has_prefix(argument, "//")) {
                g_set_error_literal(error, G_IO_ERROR, G_IO_ERROR_INVALID_ARGUMENT,
                                    "Double-leading-slash paths cannot identify a local URL selection");
                g_strfreev(paths);
                return NULL;
            }
            paths[i] = g_strdup(argument);
            continue;
        }
        gchar *scheme = argument != NULL ? g_uri_parse_scheme(argument) : NULL;
        gboolean file_scheme = scheme != NULL && g_ascii_strcasecmp(scheme, "file") == 0;
        g_free(scheme);
        if (!file_scheme || strchr(argument, '?') != NULL || strchr(argument, '#') != NULL) {
            g_set_error_literal(error, G_IO_ERROR, G_IO_ERROR_INVALID_ARGUMENT,
                                "Every selected URL must be a local file URL without a query or fragment");
            g_strfreev(paths);
            return NULL;
        }
        gchar *hostname = NULL;
        paths[i] = g_filename_from_uri(argument, &hostname, error);
        if (paths[i] == NULL) {
            g_free(hostname);
            g_strfreev(paths);
            return NULL;
        }
        /* Never treat remote authorities, credentials, or ports as local paths. */
        gboolean local = hostname == NULL || *hostname == '\0' ||
            g_ascii_strcasecmp(hostname, "localhost") == 0;
        g_free(hostname);
        if (!local) {
            g_set_error_literal(error, G_IO_ERROR, G_IO_ERROR_INVALID_ARGUMENT,
                                "Remote file URL authorities are not supported");
            g_strfreev(paths);
            return NULL;
        }
    }
    gchar **validated = fragile_selection_from_paths((const gchar *const *) paths, count, error);
    g_strfreev(paths);
    return validated;
}

gchar **
fragile_build_argv(const gchar *executable, const gchar *const *paths,
                  gsize count, GError **error)
{
    if (!validate_path(executable, error) || !validate_count(count, error))
        return NULL;
    if (paths == NULL) {
        g_set_error_literal(error, G_IO_ERROR, G_IO_ERROR_INVALID_ARGUMENT,
                            "Selection paths are missing");
        return NULL;
    }
    gchar **argv = g_new0(gchar *, count + 3);
    argv[0] = g_strdup(executable);
    argv[1] = g_strdup("--");
    for (gsize i = 0; i < count; ++i) {
        if (!validate_path(paths[i], error)) {
            g_strfreev(argv);
            return NULL;
        }
        argv[i + 2] = g_strdup(paths[i]);
    }
    return argv;
}

GSubprocess *
fragile_launch(const gchar *executable, const gchar *const *paths,
               gsize count, GError **error)
{
    gchar **argv = fragile_build_argv(executable, paths, count, error);
    if (argv == NULL)
        return NULL;
    GSubprocess *process = g_subprocess_newv((const gchar *const *) argv,
                                           G_SUBPROCESS_FLAGS_NONE, error);
    g_strfreev(argv);
    return process;
}
