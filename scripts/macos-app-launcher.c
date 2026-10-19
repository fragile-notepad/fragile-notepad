// LaunchServices needs a native executable, particularly on Apple Silicon
// systems without Rosetta. Keep runtime setup in the portable shell launcher.
#include <errno.h>
#include <mach-o/dyld.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static char *application_executable_path(void) {
    uint32_t size = 0;
    (void)_NSGetExecutablePath(NULL, &size);
    char *path = malloc(size);
    if (path == NULL) {
        perror("Fragile Notepad: allocate executable path");
        return NULL;
    }
    if (_NSGetExecutablePath(path, &size) != 0) {
        fprintf(stderr, "Fragile Notepad: cannot locate application executable\n");
        free(path);
        return NULL;
    }
    char *executable = realpath(path, NULL);
    if (executable == NULL) {
        perror("Fragile Notepad: resolve executable path");
    }
    free(path);
    return executable;
}

static char *runtime_launcher_path(const char *executable) {
    const char *directory_end = strrchr(executable, '/');
    if (directory_end == NULL) {
        fprintf(stderr, "Fragile Notepad: invalid application executable path\n");
        return NULL;
    }
    const char suffix[] = "/../Resources/runtime/fragile-notepad";
    size_t directory_length = (size_t)(directory_end - executable);
    if (directory_length > SIZE_MAX - sizeof(suffix)) {
        fprintf(stderr, "Fragile Notepad: runtime launcher path is too long\n");
        return NULL;
    }
    size_t launcher_size = directory_length + sizeof(suffix);
    char *launcher = malloc(launcher_size);
    if (launcher == NULL) {
        perror("Fragile Notepad: allocate launcher path");
        return NULL;
    }
    memcpy(launcher, executable, directory_length);
    memcpy(launcher + directory_length, suffix, sizeof(suffix));
    return launcher;
}

static int launch_runtime(char *launcher, int argc, char **argv) {
    char **arguments = calloc((size_t)argc + 2, sizeof(*arguments));
    if (arguments == NULL) {
        perror("Fragile Notepad: allocate launch arguments");
        return EXIT_FAILURE;
    }
    arguments[0] = "/bin/bash";
    arguments[1] = launcher;
    for (int index = 1; index < argc; ++index) {
        arguments[index + 1] = argv[index];
    }
    execv(arguments[0], arguments);
    int error = errno;
    fprintf(stderr, "Fragile Notepad: cannot launch application: %s\n", strerror(error));
    free(arguments);
    return EXIT_FAILURE;
}

int main(int argc, char **argv) {
    char *executable = application_executable_path();
    if (executable == NULL) {
        return EXIT_FAILURE;
    }
    char *launcher = runtime_launcher_path(executable);
    free(executable);
    if (launcher == NULL) {
        return EXIT_FAILURE;
    }
    int result = launch_runtime(launcher, argc, argv);
    free(launcher);
    return result;
}
