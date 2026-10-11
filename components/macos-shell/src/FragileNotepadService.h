#import <AppKit/AppKit.h>

NS_ASSUME_NONNULL_BEGIN

FOUNDATION_EXPORT NSString *const FNServicePortName;
FOUNDATION_EXPORT NSString *const FNLegacyFilenamesType;
FOUNDATION_EXPORT const NSUInteger FNMaximumSelectionCount;

// Validate the received file selection and the configured editor path.
FOUNDATION_EXPORT NSArray<NSString *> *_Nullable FNPathsFromPasteboard(
    NSPasteboard *pasteboard, NSString *_Nullable *_Nullable error);
FOUNDATION_EXPORT NSString *_Nullable FNExecutablePathFromBundle(
    NSBundle *bundle, NSString *_Nullable *_Nullable error);

@interface FragileNotepadService : NSObject
- (instancetype)initWithExecutablePath:(NSString *)executablePath;
- (void)editFiles:(NSPasteboard *)pasteboard
         userData:(nullable NSString *)userData
            error:(NSString *_Nullable *_Nullable)error;
@end

NS_ASSUME_NONNULL_END
