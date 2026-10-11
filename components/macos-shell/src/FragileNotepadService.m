#import "FragileNotepadService.h"
#include <sys/stat.h>

NSString *const FNServicePortName = @"FragileNotepadFinderServices";
NSString *const FNLegacyFilenamesType = @"NSFilenamesPboardType";
const NSUInteger FNMaximumSelectionCount = 4096;

static void FNSetError(NSString **error, NSString *message) {
    if (error != NULL) {
        *error = message;
    }
}

static BOOL FNIsNativeAbsolutePath(id candidate) {
    if (![candidate isKindOfClass:[NSString class]]) {
        return NO;
    }
    NSString *path = candidate;
    if (path.length == 0 || ![path hasPrefix:@"/"]) {
        return NO;
    }
    for (NSUInteger index = 0; index < path.length; index++) {
        if ([path characterAtIndex:index] == 0) {
            return NO;
        }
    }
    return path.fileSystemRepresentation != NULL;
}

static BOOL FNIsRegularFile(NSString *path) {
    struct stat attributes;
    // stat follows file symlinks, but rejects folders, broken links and devices.
    return stat(path.fileSystemRepresentation, &attributes) == 0 && S_ISREG(attributes.st_mode);
}

static BOOL FNIsLocalFileURL(NSURL *url) {
    return url != nil && url.isFileURL &&
        (url.host.length == 0 || [url.host caseInsensitiveCompare:@"localhost"] == NSOrderedSame) &&
        url.user == nil && url.password == nil && url.port == nil &&
        url.query == nil && url.fragment == nil;
}

NSArray<NSString *> *FNPathsFromPasteboard(NSPasteboard *pasteboard, NSString **error) {
    FNSetError(error, nil);
    NSArray<NSPasteboardItem *> *items = pasteboard.pasteboardItems;
    if (items == nil) {
        FNSetError(error, @"Could not read the complete file selection from Finder.");
        return nil;
    }
    NSMutableArray<NSString *> *paths = [NSMutableArray array];
    BOOL hasFileURLs = NO;
    for (NSPasteboardItem *item in items) {
        if ([item.types containsObject:NSPasteboardTypeFileURL]) {
            hasFileURLs = YES;
            break;
        }
    }

    if (hasFileURLs) {
        if (items.count > FNMaximumSelectionCount) {
            FNSetError(error, @"Select no more than 4096 files.");
            return nil;
        }
        // Inspect every item. readObjectsForClasses can silently omit unsupported items.
        for (NSPasteboardItem *item in items) {
            NSString *representation = [item stringForType:NSPasteboardTypeFileURL];
            NSURL *url = representation == nil ? nil : [NSURL URLWithString:representation];
            if (!FNIsLocalFileURL(url) || !FNIsNativeAbsolutePath(url.path)) {
                FNSetError(error, @"The entire selection must contain local filesystem files.");
                return nil;
            }
            [paths addObject:url.path];
        }
    } else {
        id filenames = [pasteboard propertyListForType:FNLegacyFilenamesType];
        if (![filenames isKindOfClass:[NSArray class]]) {
            FNSetError(error, @"Select local filesystem files in Finder.");
            return nil;
        }
        if ([filenames count] > FNMaximumSelectionCount) {
            FNSetError(error, @"Select no more than 4096 files.");
            return nil;
        }
        // A legacy file list is one pasteboard item; extra items must not be discarded.
        if (items.count > 1) {
            FNSetError(error, @"The entire selection must contain local filesystem files.");
            return nil;
        }
        for (id path in filenames) {
            if (!FNIsNativeAbsolutePath(path)) {
                FNSetError(error, @"The entire selection must contain absolute filesystem paths.");
                return nil;
            }
            [paths addObject:path];
        }
    }

    if (paths.count == 0) {
        FNSetError(error, @"Select at least one file.");
        return nil;
    }
    for (NSString *path in paths) {
        if (!FNIsRegularFile(path)) {
            FNSetError(error, @"The entire selection must contain existing files, without folders or special devices.");
            return nil;
        }
    }
    return [paths copy];
}

NSString *FNExecutablePathFromBundle(NSBundle *bundle, NSString **error) {
    FNSetError(error, nil);
    id path = [bundle objectForInfoDictionaryKey:@"FragileNotepadExecutable"];
    if (!FNIsNativeAbsolutePath(path)) {
        FNSetError(error, @"The service must be configured with an absolute Fragile Notepad executable path.");
        return nil;
    }
    return path;
}

@interface FragileNotepadService ()
@property(nonatomic, copy) NSString *executablePath;
@end

@implementation FragileNotepadService
- (instancetype)initWithExecutablePath:(NSString *)executablePath {
    self = [super init];
    if (self != nil) {
        _executablePath = [executablePath copy];
    }
    return self;
}

- (void)editFiles:(NSPasteboard *)pasteboard userData:(NSString *)userData error:(NSString **)error {
    (void)userData;
    FNSetError(error, nil);
    NSArray<NSString *> *paths = FNPathsFromPasteboard(pasteboard, error);
    if (paths == nil) {
        return;
    }
    if (!FNIsNativeAbsolutePath(self.executablePath) || !FNIsRegularFile(self.executablePath) ||
        ![[NSFileManager defaultManager] isExecutableFileAtPath:self.executablePath]) {
        FNSetError(error, @"The configured Fragile Notepad executable is missing or cannot be executed.");
        return;
    }

    NSMutableArray<NSString *> *arguments = [NSMutableArray arrayWithObject:@"--"];
    [arguments addObjectsFromArray:paths];
    NSTask *task = [[NSTask alloc] init];
    task.executableURL = [NSURL fileURLWithPath:self.executablePath];
    task.arguments = arguments;
    // Launch exactly once after full validation, and never wait for the editor to exit.
    NSError *launchError = nil;
    @try {
        if (![task launchAndReturnError:&launchError]) {
            FNSetError(error, [NSString stringWithFormat:@"Could not start Fragile Notepad: %@",
                launchError.localizedDescription ?: @"unknown launch error"]);
        }
    } @catch (NSException *exception) {
        FNSetError(error, [NSString stringWithFormat:@"Could not start Fragile Notepad: %@",
            exception.reason ?: exception.name]);
    }
}
@end
