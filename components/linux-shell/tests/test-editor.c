#include <gio/gio.h>
#include <string.h>

/* Native argv recorder for process tests. No shell or editor dependency. */
int
main(int argc, char **argv)
{
    const gchar *output = g_getenv("FRAGILE_SHELL_TEST_OUTPUT");
    if (output == NULL)
        return 2;
    GString *record = g_string_new(NULL);
    for (int i = 1; i < argc; ++i)
        g_string_append_len(record, argv[i], strlen(argv[i]) + 1);
    GError *error = NULL;
    gboolean written = g_file_set_contents(output, record->str, (gssize) record->len, &error);
    g_string_free(record, TRUE);
    if (!written) {
        g_printerr("%s\n", error->message);
        g_error_free(error);
        return 1;
    }
    return 0;
}
