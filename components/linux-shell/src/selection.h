#ifndef FRAGILE_SHELL_SELECTION_H
#define FRAGILE_SHELL_SELECTION_H

#include <gio/gio.h>

#define FRAGILE_SHELL_MAX_FILES 4096u

typedef struct {
    const gchar *path;
    GFileType type;
    gboolean native;
    gboolean gone;
} FragileSelectionItem;

/* Menu callbacks use cached metadata only. The returned vector owns its paths. */
gchar **fragile_selection_from_items(const FragileSelectionItem *items,
                                    gsize count, GError **error);
/* Activation validates every native path again, following links to regular files. */
gchar **fragile_selection_from_paths(const gchar *const *paths,
                                    gsize count, GError **error);
/* Dolphin %U keeps original URL provenance; absolute native paths are also valid. */
gchar **fragile_selection_from_file_arguments(const gchar *const *arguments,
                                             gsize count, GError **error);
gchar **fragile_build_argv(const gchar *executable,
                         const gchar *const *paths, gsize count, GError **error);
/* One process; no shell, PATH search, or truncation. Caller owns the process. */
GSubprocess *fragile_launch(const gchar *executable,
                           const gchar *const *paths, gsize count, GError **error);

#endif
