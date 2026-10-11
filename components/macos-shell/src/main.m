#import "FragileNotepadService.h"
#include <stdio.h>

int main(void) {
    @autoreleasepool {
        NSString *error = nil;
        NSString *executablePath = FNExecutablePathFromBundle([NSBundle mainBundle], &error);
        if (executablePath == nil) {
            fprintf(stderr, "%s\n", error.UTF8String);
            return 1;
        }
        // Requests can arrive as soon as this registration completes.
        __attribute__((objc_precise_lifetime)) FragileNotepadService *provider = [[FragileNotepadService alloc]
            initWithExecutablePath:executablePath];
        NSRegisterServicesProvider(provider, FNServicePortName);
        NSRunLoop *runLoop = [NSRunLoop currentRunLoop];
        while (YES) {
            // A Foundation tool must drain temporary objects between service requests.
            @autoreleasepool {
                if (![runLoop runMode:NSDefaultRunLoopMode beforeDate:[NSDate distantFuture]]) {
                    break;
                }
            }
        }
    }
    return 0;
}
