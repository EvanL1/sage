#import <UserNotifications/UserNotifications.h>
#import <AppKit/AppKit.h>

// Rust 侧注册的回调：通知被点击时调用，传入路由路径
static void (*g_on_click)(const char* route) = NULL;
static BOOL g_use_legacy = NO; // YES = 降级到 NSUserNotificationCenter

// ─── 现代 API：UNUserNotificationCenter（需要签名）──────────────────

@interface SageNotificationDelegate : NSObject <UNUserNotificationCenterDelegate>
@end

@implementation SageNotificationDelegate

- (void)userNotificationCenter:(UNUserNotificationCenter *)center
       willPresentNotification:(UNNotification *)notification
         withCompletionHandler:(void (^)(UNNotificationPresentationOptions))handler {
    handler(UNNotificationPresentationOptionBanner | UNNotificationPresentationOptionSound);
}

- (void)userNotificationCenter:(UNUserNotificationCenter *)center
didReceiveNotificationResponse:(UNNotificationResponse *)response
         withCompletionHandler:(void (^)(void))handler {
    NSString *route = response.notification.request.content.userInfo[@"route"];
    if (route && g_on_click) {
        g_on_click([route UTF8String]);
    }
    handler();
}

@end

// ─── 降级 API：NSUserNotificationCenter（废弃但不需要签名）────────

#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"

@interface SageLegacyDelegate : NSObject <NSUserNotificationCenterDelegate>
@end

@implementation SageLegacyDelegate

// 前台也弹通知
- (BOOL)userNotificationCenter:(NSUserNotificationCenter *)center
     shouldPresentNotification:(NSUserNotification *)notification {
    return YES;
}

// 用户点击通知
- (void)userNotificationCenter:(NSUserNotificationCenter *)center
       didActivateNotification:(NSUserNotification *)notification {
    NSString *route = notification.userInfo[@"route"];
    if (route && g_on_click) {
        g_on_click([route UTF8String]);
    }
}

@end

#pragma clang diagnostic pop

// ─── 公共接口 ────────────────────────────────────────────────────

static SageNotificationDelegate *g_delegate = nil;
static SageLegacyDelegate *g_legacy_delegate = nil;

void sage_notification_init(void (*on_click)(const char*)) {
    g_on_click = on_click;

    // 先尝试现代 API
    UNUserNotificationCenter *center = [UNUserNotificationCenter currentNotificationCenter];
    g_delegate = [[SageNotificationDelegate alloc] init];
    center.delegate = g_delegate;

    dispatch_semaphore_t sem = dispatch_semaphore_create(0);
    __block BOOL authOK = NO;

    [center requestAuthorizationWithOptions:(UNAuthorizationOptionAlert |
                                             UNAuthorizationOptionSound |
                                             UNAuthorizationOptionBadge)
                          completionHandler:^(BOOL granted, NSError *error) {
        authOK = granted && !error;
        if (error) {
            NSLog(@"Sage: UNUserNotificationCenter auth failed (code=%ld), will use legacy API",
                  (long)error.code);
        }
        dispatch_semaphore_signal(sem);
    }];

    // 等待授权结果（最多 2 秒）
    dispatch_semaphore_wait(sem, dispatch_time(DISPATCH_TIME_NOW, 2 * NSEC_PER_SEC));

    if (!authOK) {
        g_use_legacy = YES;
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
        g_legacy_delegate = [[SageLegacyDelegate alloc] init];
        [NSUserNotificationCenter defaultUserNotificationCenter].delegate = g_legacy_delegate;
#pragma clang diagnostic pop
        NSLog(@"Sage: using legacy NSUserNotificationCenter (no signing required)");
    } else {
        NSLog(@"Sage: using UNUserNotificationCenter");
    }
}

void sage_notification_send(const char* title, const char* body, const char* route) {
    if (g_use_legacy) {
        // Legacy API
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
        NSUserNotification *n = [[NSUserNotification alloc] init];
        n.title = [NSString stringWithUTF8String:title];
        n.informativeText = [NSString stringWithUTF8String:body];
        n.soundName = NSUserNotificationDefaultSoundName;
        n.userInfo = @{@"route": [NSString stringWithUTF8String:route]};
        [[NSUserNotificationCenter defaultUserNotificationCenter] deliverNotification:n];
#pragma clang diagnostic pop
        return;
    }

    // Modern API
    UNMutableNotificationContent *content = [[UNMutableNotificationContent alloc] init];
    content.title = [NSString stringWithUTF8String:title];
    content.body = [NSString stringWithUTF8String:body];
    content.sound = [UNNotificationSound defaultSound];
    content.userInfo = @{@"route": [NSString stringWithUTF8String:route]};

    NSString *identifier = [[NSUUID UUID] UUIDString];
    UNNotificationRequest *request = [UNNotificationRequest requestWithIdentifier:identifier
                                                                          content:content
                                                                          trigger:nil];

    [[UNUserNotificationCenter currentNotificationCenter] addNotificationRequest:request
                                                           withCompletionHandler:^(NSError *error) {
        if (error) {
            NSLog(@"Sage notification send error: %@", error);
        }
    }];
}
