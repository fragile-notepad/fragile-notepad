#include <nautilus-extension.h>
#include <gmodule.h>
#include "config.h"
#include "selection.h"

typedef struct { GObject parent; } FragileNotepadMenu;
typedef struct { GObjectClass parent; } FragileNotepadMenuClass;

static void fragile_menu_provider_init(NautilusMenuProviderInterface *iface);

G_DEFINE_DYNAMIC_TYPE_EXTENDED(FragileNotepadMenu, fragile_notepad_menu,
    G_TYPE_OBJECT, 0,
    G_IMPLEMENT_INTERFACE_DYNAMIC(NAUTILUS_TYPE_MENU_PROVIDER, fragile_menu_provider_init))

static void
fragile_notepad_menu_class_init(FragileNotepadMenuClass *klass)
{
    (void) klass;
}

static void
fragile_notepad_menu_class_finalize(FragileNotepadMenuClass *klass)
{
    (void) klass;
}

static void
fragile_notepad_menu_init(FragileNotepadMenu *self)
{
    (void) self;
}

static void
activate(NautilusMenuItem *item, gpointer user_data)
{
    (void) user_data;
    const gchar *const *saved = g_object_get_data(G_OBJECT(item), "fragile-selected-paths");
    gsize count = g_strv_length((gchar **) saved);
    GError *error = NULL;
    gchar **paths = fragile_selection_from_paths(saved, count, &error);
    GSubprocess *process = paths != NULL
        ? fragile_launch(FRAGILE_NOTEPAD_EXECUTABLE, (const gchar *const *) paths, count, &error)
        : NULL;
    g_strfreev(paths);
    if (process == NULL) {
        g_warning("Fragile Notepad: unable to open selection: %s", error->message);
        g_error_free(error);
        return;
    }
    g_object_unref(process);
}

static GList *
get_file_items(NautilusMenuProvider *provider, GList *files)
{
    /* Bound the walk before allocating, and never return a partial selection. */
    gsize count = 0;
    for (GList *node = files; node != NULL; node = node->next) {
        if (++count > FRAGILE_SHELL_MAX_FILES)
            return NULL;
    }
    if (count == 0)
        return NULL;
    FragileSelectionItem *selection = g_new0(FragileSelectionItem, count);
    gsize index = 0;
    for (GList *node = files; node != NULL; node = node->next, ++index) {
        if (!NAUTILUS_IS_FILE_INFO(node->data))
            continue;
        NautilusFileInfo *info = NAUTILUS_FILE_INFO(node->data);
        GFile *location = nautilus_file_info_get_location(info);
        selection[index].type = nautilus_file_info_get_file_type(info);
        selection[index].gone = nautilus_file_info_is_gone(info);
        selection[index].native = location != NULL && g_file_is_native(location);
        selection[index].path = location != NULL ? g_file_get_path(location) : NULL;
        g_clear_object(&location);
    }
    gchar **paths = fragile_selection_from_items(selection, count, NULL);
    for (gsize i = 0; i < count; ++i)
        g_free((gpointer) selection[i].path);
    g_free(selection);
    if (paths == NULL)
        return NULL;
    NautilusMenuItem *item = nautilus_menu_item_new(
        "FragileNotepad::Edit", "Edit with Fragile Notepad",
        "Open selected files in Fragile Notepad", "accessories-text-editor");
    g_object_set_data_full(G_OBJECT(item), "fragile-selected-paths", paths, (GDestroyNotify) g_strfreev);
    /* Keep this dynamic module alive as long as the item can call activate. */
    g_object_set_data_full(G_OBJECT(item), "fragile-menu-provider", g_object_ref(provider), g_object_unref);
    g_signal_connect(item, "activate", G_CALLBACK(activate), NULL);
    return g_list_append(NULL, item);
}

static void
fragile_menu_provider_init(NautilusMenuProviderInterface *iface)
{
    iface->get_file_items = get_file_items;
}

G_MODULE_EXPORT void
nautilus_module_initialize(GTypeModule *module)
{
    fragile_notepad_menu_register_type(module);
}

G_MODULE_EXPORT void
nautilus_module_shutdown(void)
{
}

G_MODULE_EXPORT void
nautilus_module_list_types(const GType **types, int *num_types)
{
    static GType exported[1];
    exported[0] = fragile_notepad_menu_get_type();
    *types = exported;
    *num_types = 1;
}
