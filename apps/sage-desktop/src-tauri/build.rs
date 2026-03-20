fn main() {
    tauri_build::build();

    // 编译 Objective-C 通知模块
    cc::Build::new()
        .file("src/notification.m")
        .flag("-fobjc-arc")
        .compile("sage_notification");

    println!("cargo:rustc-link-lib=framework=UserNotifications");
    println!("cargo:rustc-link-lib=framework=AppKit");
}
