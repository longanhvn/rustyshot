# Changelog

Tất cả các thay đổi đáng chú ý của **RustyShot** được ghi nhận tại đây.

## [0.3.11] - 2026-11-25
### Đã thêm / Cải thiện
- Hỗ trợ **SFTP / SSH (Port 22)** bên cạnh **FTPS Explicit TLS** và **FTP thường** trong tab **Upload & FTP**.
- Thêm timeout kết nối TCP rõ ràng (10s) và bật `passive_nat_workaround` cho FTP/FTPS.

## [0.3.10] - 2026-11-25
### Đã thêm / Thay đổi
- **Tích hợp sẵn bộ mã hóa video MP4 (H.264)** ngay bên trong ứng dụng (`openh264` + `mp4`), quay màn hình MP4 trực tiếp mà **không cần cài đặt `ffmpeg` / `ffmpeg.exe` ngoài**.
- Gỡ bỏ dòng thông tin tác giả khỏi tài liệu và metadata phát hành.

## [0.3.9] - 2026-11-25
### Đã thêm
- Thiết kế và nhúng **Icon chính thức** cho ứng dụng (`assets/icon.ico` & `assets/icon.png`): hiển thị đồng bộ trên file `.exe` Windows (Explorer, Taskbar, Alt+Tab), cửa sổ ứng dụng và System Tray.
- Bổ sung ảnh chụp giao diện thực tế vào `README.md` (`docs/screenshots/rustyshot-dashboard.png`).

## [0.3.8] - 2026-11-25
### Đã cải thiện
- Nâng cấp thanh công cụ trong **Editor**: chuyển sang bố cục tự động xuống dòng (`horizontal_wrapped`) để không bị tràn/cắt nút khi cửa sổ nhỏ, đồng thời tăng kích thước nút, chữ (`13.5pt`) và chấm màu (`18×18`) giúp thao tác dễ hơn.

## [0.3.7] - 2026-11-25
### Đã cải thiện
- Chuyển thanh công cụ nổi sau khi chọn vùng chụp từ cạnh dưới lên **cạnh trên giữa màn hình** (`CENTER_TOP`).
- Phóng to kích thước thanh công cụ nổi: tăng lề, khoảng cách nút, cỡ chữ/icon (`14.5–15pt`) và chấm chọn màu (`20×20`) để dễ thao tác hơn.

## [0.3.6] - 2026-11-25
### Đã thêm / Sửa lỗi
- Hỗ trợ **FTPS (Explicit TLS)** thuần Rust (`suppaftp` + `rustls`), giải quyết lỗi không kết nối/upload được với các server yêu cầu `AUTH TLS`.
- Thêm tùy chọn `🔒 FTPS (Explicit TLS)` và `Accept self-signed certs` trong tab **Upload & FTP**.
- Cải thiện `CWD` thư mục từ xa: thử chuyển thẳng vào đường dẫn đích trước và tự động xử lý trường hợp tài khoản FTP bị chroot `/home/<username>`.

## [0.3.5] - 2026-11-25
### Đã sửa
- Khắc phục triệt để lỗi không mở lại được cửa sổ RustyShot từ System Tray trên Windows khi cửa sổ đang ẩn (`SW_HIDE` làm ngừng `WM_PAINT`): chuyển lắng nghe sự kiện Tray, Hotkey và Single-Instance sang luồng nền độc lập và gọi trực tiếp Win32 `ShowWindow` / `SetForegroundWindow` lên `HWND`.

## [0.3.4] - 2026-11-25
### Đã thêm
- Cơ chế **Single Instance** (`127.0.0.1:47831`): ngăn mở nhiều tiến trình cùng lúc, mở lần 2 tự động đánh thức cửa sổ đang chạy.
- Hỗ trợ click chuột trái một lần vào icon System Tray để mở lại cửa sổ.

## [0.3.3] - 2026-11-25
### Đã thay đổi
- Chuyển lưu trữ sang 100% Portable ngay cạnh file thực thi: `./data/RustyShot.json`, `./data/image/` và `./data/video/`, tự động chuyển đổi cấu hình cũ.

## [0.3.2] - 2026-11-25
### Đã sửa
- Rút gọn thông báo xung đột phím tắt trên thanh tiêu đề (nút `⚠ Hotkey conflict`), tránh tràn chữ đè lên logo.
- Căn đều lưới 3 cột thẻ tác vụ nhanh và tăng độ rõ viền checkbox.

## [0.3.1] - 2026-11-25
### Đã thêm
- Giao diện điều hướng thanh bên trái 5 tab (`Capture`, `Tools`, `Upload & FTP`, `History`, `Settings`), 6 thẻ tác vụ nhanh và thanh công tắc `After Capture`.
- Color Picker (lấy mã màu + lịch sử màu), thanh nổi khi quay màn hình (`Ctrl+Shift+R`), dán ảnh `Ctrl+V` / kéo thả file.
- Nâng cấp Editor: Zoom (`-`/`100%`/`+`/`Fit`/`Ctrl+Cuộn`), xoay/lật ảnh, cắt ảnh (Crop), thước căn dòng, giữ `Shift` khóa vuông, phím mũi tên dịch `1px`/`10px`.
- Nâng cấp `PinnedShot`: hỗ trợ zoom và chỉnh độ mờ (Opacity).

## [0.1.0 - 0.3.0] - 2026-11-23 -> 2026-11-25
### Khởi tạo
- Xây dựng lõi chụp màn hình đa nền tảng, chọn vùng với kính lúp 11×11, trình chú thích 9 công cụ + Undo/Redo, ghim ảnh nổi, quay màn hình GIF/MP4, upload FTP và System Tray đa nền tảng.
