#include "config.h"
#include "selection.h"

int
main(int argc, char **argv)
{
    gboolean file_urls = argc > 1 && g_strcmp0(argv[1], "--file-urls") == 0;
    int first_file = file_urls ? 3 : 2;
    if (argc <= first_file || g_strcmp0(argv[first_file - 1], "--") != 0) {
        g_printerr("Fragile Notepad: expected [--file-urls] -- followed by selected local files\n");
        return 2;
    }
    GError *error = NULL;
    gsize count = (gsize) argc - first_file;
    const gchar *const *arguments = (const gchar *const *) argv + first_file;
    gchar **paths = file_urls
        ? fragile_selection_from_file_arguments(arguments, count, &error)
        : fragile_selection_from_paths(arguments, count, &error);
    GSubprocess *process = paths != NULL
        ? fragile_launch(FRAGILE_NOTEPAD_EXECUTABLE, (const gchar *const *) paths, count, &error)
        : NULL;
    g_strfreev(paths);
    if (process == NULL) {
        g_printerr("Fragile Notepad: unable to open selection: %s\n", error->message);
        g_error_free(error);
        return 1;
    }
    g_object_unref(process);
    return 0;
}
