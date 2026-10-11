#include <nautilus-extension.h>
#include <gmodule.h>

typedef struct {
    GObject parent;
    GFile *location;
    GFileType file_type;
    gboolean gone;
} TestFileInfo;
typedef struct { GObjectClass parent; } TestFileInfoClass;

static void test_file_info_interface_init(NautilusFileInfoInterface *iface);
G_DEFINE_TYPE_WITH_CODE(TestFileInfo, test_file_info, G_TYPE_OBJECT,
    G_IMPLEMENT_INTERFACE(NAUTILUS_TYPE_FILE_INFO, test_file_info_interface_init))

static GFile *
get_location(NautilusFileInfo *info)
{
    TestFileInfo *file = (TestFileInfo *) info;
    return file->location != NULL ? g_object_ref(file->location) : NULL;
}

static GFileType
get_file_type(NautilusFileInfo *info)
{
    return ((TestFileInfo *) info)->file_type;
}

static gboolean
is_gone(NautilusFileInfo *info)
{
    return ((TestFileInfo *) info)->gone;
}

static void
test_file_info_interface_init(NautilusFileInfoInterface *iface)
{
    iface->get_location = get_location;
    iface->get_file_type = get_file_type;
    iface->is_gone = is_gone;
}

static void
test_file_info_finalize(GObject *object)
{
    g_clear_object(&((TestFileInfo *) object)->location);
    G_OBJECT_CLASS(test_file_info_parent_class)->finalize(object);
}

static void
test_file_info_class_init(TestFileInfoClass *klass)
{
    G_OBJECT_CLASS(klass)->finalize = test_file_info_finalize;
}

static void
test_file_info_init(TestFileInfo *self)
{
    self->file_type = G_FILE_TYPE_REGULAR;
}

typedef struct {
    GTypeModule parent;
    GModule *library;
    const gchar *filename;
    void (*list_types)(const GType **, int *);
    void (*shutdown)(void);
} TestModule;
typedef struct { GTypeModuleClass parent; } TestModuleClass;
G_DEFINE_TYPE(TestModule, test_module, G_TYPE_TYPE_MODULE)

static gboolean
test_module_load(GTypeModule *type_module)
{
    TestModule *module = (TestModule *) type_module;
    module->library = g_module_open(module->filename, G_MODULE_BIND_LOCAL);
    g_assert_nonnull(module->library);
    void (*initialize)(GTypeModule *) = NULL;
    g_assert_true(g_module_symbol(module->library, "nautilus_module_initialize", (gpointer *) &initialize));
    g_assert_true(g_module_symbol(module->library, "nautilus_module_list_types", (gpointer *) &module->list_types));
    g_assert_true(g_module_symbol(module->library, "nautilus_module_shutdown", (gpointer *) &module->shutdown));
    initialize(type_module);
    return TRUE;
}

static void
test_module_unload(GTypeModule *type_module)
{
    TestModule *module = (TestModule *) type_module;
    module->shutdown();
    g_module_close(module->library);
    module->library = NULL;
}

static void
test_module_class_init(TestModuleClass *klass)
{
    G_TYPE_MODULE_CLASS(klass)->load = test_module_load;
    G_TYPE_MODULE_CLASS(klass)->unload = test_module_unload;
}

static void
test_module_init(TestModule *self)
{
    (void) self;
}

static NautilusMenuProvider *provider;

static TestFileInfo *
new_file(const gchar *uri, GFileType type, gboolean gone)
{
    TestFileInfo *file = g_object_new(test_file_info_get_type(), NULL);
    file->location = uri != NULL ? g_file_new_for_uri(uri) : NULL;
    file->file_type = type;
    file->gone = gone;
    return file;
}

static void
test_native_menu(void)
{
    TestFileInfo *file = new_file("file:///tmp/space%20%25%27%24.txt", G_FILE_TYPE_REGULAR, FALSE);
    GList *files = g_list_append(NULL, file);
    GList *items = nautilus_menu_provider_get_file_items(provider, files);
    g_assert_cmpuint(g_list_length(items), ==, 1);
    g_assert_true(NAUTILUS_IS_MENU_ITEM(items->data));
    gchar *label = NULL;
    g_object_get(items->data, "label", &label, NULL);
    g_assert_cmpstr(label, ==, "Edit with Fragile Notepad");
    g_free(label);
    const gchar *const *saved = g_object_get_data(items->data, "fragile-selected-paths");
    g_assert_cmpstr(saved[0], ==, "/tmp/space %'$.txt");
    g_assert_null(saved[1]);
    g_assert_true(g_object_get_data(items->data, "fragile-menu-provider") == provider);
    /* Borrowed FileInfo objects can disappear without invalidating the item. */
    g_list_free_full(files, g_object_unref);
    g_assert_cmpstr(saved[0], ==, "/tmp/space %'$.txt");
    g_list_free_full(items, g_object_unref);
}

static void
test_invalid_menu(void)
{
    g_assert_null(nautilus_menu_provider_get_file_items(provider, NULL));
    struct {
        const gchar *uri;
        GFileType type;
        gboolean gone;
    } invalid[] = {
        {"file:///tmp/folder", G_FILE_TYPE_DIRECTORY, FALSE},
        {"smb://example.invalid/share/file", G_FILE_TYPE_REGULAR, FALSE},
        {"file:///tmp/gone", G_FILE_TYPE_REGULAR, TRUE},
        {NULL, G_FILE_TYPE_REGULAR, FALSE},
        {"file:///tmp/unknown", G_FILE_TYPE_UNKNOWN, FALSE}
    };
    for (gsize i = 0; i < G_N_ELEMENTS(invalid); ++i) {
        GList *files = g_list_append(NULL, new_file("file:///tmp/valid", G_FILE_TYPE_REGULAR, FALSE));
        files = g_list_append(files, new_file(invalid[i].uri, invalid[i].type, invalid[i].gone));
        g_assert_null(nautilus_menu_provider_get_file_items(provider, files));
        g_list_free_full(files, g_object_unref);
    }
}

static void
test_menu_limit(void)
{
    GList *files = NULL;
    TestFileInfo *file = new_file("file:///tmp/file", G_FILE_TYPE_REGULAR, FALSE);
    for (gsize i = 0; i < 4096; ++i)
        files = g_list_prepend(files, file);
    GList *items = nautilus_menu_provider_get_file_items(provider, files);
    g_assert_cmpuint(g_list_length(items), ==, 1);
    const gchar *const *saved = g_object_get_data(items->data, "fragile-selected-paths");
    g_assert_cmpuint(g_strv_length((gchar **) saved), ==, 4096);
    g_list_free_full(items, g_object_unref);
    files = g_list_prepend(files, file);
    g_assert_null(nautilus_menu_provider_get_file_items(provider, files));
    g_list_free(files);
    g_object_unref(file);
}

int
main(int argc, char **argv)
{
    if (argc != 2)
        return 2;
    TestModule *module = g_object_new(test_module_get_type(), NULL);
    module->filename = argv[1];
    g_assert_true(g_type_module_use(G_TYPE_MODULE(module)));
    const GType *types = NULL;
    int type_count = 0;
    module->list_types(&types, &type_count);
    g_assert_cmpint(type_count, ==, 1);
    g_assert_true(g_type_is_a(types[0], NAUTILUS_TYPE_MENU_PROVIDER));
    provider = g_object_new(types[0], NULL);
    argc = 1;
    g_test_init(&argc, &argv, NULL);
    g_test_add_func("/nautilus/native-menu-path-ownership", test_native_menu);
    g_test_add_func("/nautilus/whole-selection-filter", test_invalid_menu);
    g_test_add_func("/nautilus/4096-limit", test_menu_limit);
    int result = g_test_run();
    g_object_unref(provider);
    g_type_module_unuse(G_TYPE_MODULE(module));
    /* GTypeModule lives for registered dynamic types, as in Nautilus itself. */
    return result;
}
