# Changelog

Tất cả các thay đổi đáng chú ý của **RustyShot** được ghi nhận tại đây.

## [0.3.17] - 2026-11-27
### Tính năng mới & Sửa lỗi trọng tâm
- **Chụp phím tắt tức thì, không lộ cửa sổ & triệt tiêu 100% hiện tượng lóe trắng / chớp tắt trên Windows:**
  - Chuyển sang cơ chế chụp nền trước khi đánh thức giao diện kết hợp **Windows DWM Cloaking (`DWMWA_CLOAK`)**, tắt hiệu ứng chuyển cảnh (`DWMWA_TRANSITIONS_FORCEDISABLED`) và đồng bộ Dark Titlebar (`DWMWA_USE_IMMERSIVE_DARK_MODE`).
  - Khi bấm phím tắt chụp (kể cả khi đang mở cửa sổ chính, thu nhỏ hoặc ẩn dưới System Tray), màn hình được chụp sạch 100% không bao giờ dính cửa sổ RustyShot; lớp phủ chọn vùng chỉ gỡ Cloak sau khi OpenGL đã vẽ xong ảnh chụp ở đúng độ phân giải đích, loại bỏ hoàn toàn khung trắng và hiện tượng tự ẩn chớp tắt.
  - Ngăn kích hoạt chồng phím tắt chụp mới khi đang mở sẵn lớp phủ chọn vùng.
- **Hỗ trợ mượt mà đa màn hình & màn hình rời (Multi-Monitor Virtual Desktop):**
  - Phủ lớp chọn vùng 1:1 xuyên suốt toàn bộ không gian đa màn hình (bao gồm cả màn hình phụ đặt bên trái hoặc phía trên có tọa độ âm `X < 0`, `Y < 0`), khắc phục lỗi co ảnh kèm viền đen trên 1 màn hình.
  - Bổ sung thông tin ranh giới từng màn hình (`MonitorBounds`): thanh công cụ nổi sau khi kéo vùng chọn, dòng hướng dẫn và kính lúp soi pixel (Loupe) luôn tự bám gọn trong đúng màn hình đang thao tác, không bao giờ bị cắt ngang qua khe giữa 2 màn hình.
- **Các bản sửa lỗi quan trọng khác:**
  - **Nút `📋 Copy` & `💾 Save` trên thanh công cụ nổi luôn hoạt động độc lập:** Bấm `Copy` hoặc `Save` luôn thực thi ngay hành động tương ứng kể cả khi người dùng tắt tùy chọn tự động (`Auto-copy` / `Auto-save`) trong Settings.
  - **Giới hạn biên khi tinh chỉnh bằng phím mũi tên:** Đẩy vùng chọn `1px` / `Shift + Mũi tên` `10px` luôn tự kẹp trong phạm vi kích thước ảnh chụp.
  - **Chuẩn hóa tỷ lệ khung viền đỏ khi quay video vùng chọn trên màn hình High-DPI / đa màn hình.**

## [0.3.14] - 2026-11-25
### Sửa lỗi (Bugfixes)
- **Giữ nguyên trạng thái ẩn/hiện cửa sổ chính sau khi chụp ảnh:** Khi người dùng ẩn RustyShot xuống khay hệ thống (hoặc thu nhỏ cửa sổ) rồi chụp vùng/toàn màn hình qua phím tắt hoặc tray, bấm **Copy**, **Save**, **Pin**, **Rec** hoặc **Esc/Hủy** trên thanh công cụ chụp sẽ tự động giữ nguyên trạng thái ẩn/thu nhỏ như trước khi chụp, không tự bật lại cửa sổ chính gây bất tiện; nếu cửa sổ chính đang mở trước khi chụp thì sau khi chụp xong vẫn giữ hiện bình thường.

## [0.3.13] - 2026-11-25
### Tính năng mới & Sửa lỗi
- **Hỗ trợ đa ngôn ngữ (English & Tiếng Việt):**
  - Thêm nút chuyển nhanh **`🌐 EN` / `🌐 VI`** ngay trên góc phải thanh trên cùng và tùy chọn ngôn ngữ trong **Settings**, tự động lưu cấu hình và chuyển đổi toàn bộ giao diện tức thì không cần khởi động lại app.
- **Nạp font hệ thống Unicode (`Segoe UI` / `Tahoma` / `Noto Sans` / `SFNS`)**: khắc phục triệt để lỗi thiếu dấu tiếng Việt trên Windows, Ubuntu và macOS mà không làm tăng dung lượng file thực thi.

## [0.3.12] - 2026-11-25
### Tính năng mới & Bảo mật
- Thêm tùy chọn **Open after upload** (`After Capture` & `Upload & FTP`): tự động mở link Public URL trên trình duyệt ngay sau khi upload thành công.
- **Đổi Hotkeys kiểu bấm-trực-tiếp (Click-to-Bind) & hỗ trợ 1 phím `PrtSc`**: trong `Settings → Global Hotkeys`, chỉ cần nhấp vào ô phím tắt rồi bấm tổ hợp phím mới hoặc 1 phím `PrtSc` (`PrintScreen`) là tự nhận, tự lưu và áp dụng ngay (kèm nút `PrtSc` & `↺ Reset`).
- **Thêm mục `About RustyShot`** trong tab `Settings` hiển thị thông tin ứng dụng, phiên bản, đường dẫn repository GitHub và nút mở nhanh trang Releases.
- **Hiển thị tiến trình & trạng thái Upload cố định ở viền dưới**: thanh trạng thái (`⏳ Uploading...` → `✅ Uploaded` kèm nút `📋 Copy URL` & `🌐 Open` hoặc `❌ Lỗi`) + spinner ngay trên từng mục trong lịch sử.
- **Mã hóa mật khẩu FTP/SFTP bằng AES-256-GCM** trước khi lưu vào `data/RustyShot.json` (tự động nâng cấp mật khẩu cũ từ dạng văn bản thường sang `enc:v1:...`).
- **Quy tắc tên file chữ thường 100%**: mọi file ảnh/video sau khi chụp hoặc quay đều dùng chữ thường (`rustyshot_...png`, `rustyshot_rec_...mp4`), không bao giờ viết hoa.

### Tối ưu hiệu năng & Bất đồng bộ
- **Lưu file ảnh bất đồng bộ & Nén PNG tốc độ cao (`CompressionType::Fast` + `FilterType::Sub`)**: chụp hoặc lưu ảnh không còn khựng UI thread; sau khi ghi file xong tự động đẩy sang luồng upload nếu bật `Auto-upload`.
- **Dừng quay video bất đồng bộ**: bấm Stop tắt thanh nổi ngay và đóng gói MP4/GIF ở luồng nền, tránh treo giao diện khi quay video dài.
- **Tải thumbnail bất đồng bộ**: danh sách `Recent Captures` & `History` giải mã ảnh thu nhỏ ở luồng nền, cuộn mượt mà ngay cả khi có nhiều ảnh độ phân giải cao.

### Đã sửa
- **Sửa lỗi nút `⏹ Stop` nhỏ trên thanh nổi khi quay video không bấm dừng được**: dùng cờ nguyên tử `rec_stop_requested` gắn cố định vào app thay vì tạo mới mỗi frame và bỏ vùng kéo đè lên nút Stop.
- **Thêm nút `✂ Record Region` ngay trên màn hình chính (`Capture`)** bên cạnh **`🎬 Record Full`** để chọn quay toàn màn hình hoặc kéo chọn 1 vùng bất kỳ rồi quay (kèm khung viền đỏ trong suốt bao quanh vùng đang quay, bấm xuyên chuột được).
- **Sửa lỗi copy URL & mở link sau khi upload trên Windows**: chuyển `copy_text_to_clipboard` về UI thread chính, gọi `flush()` trước khi đóng stream SFTP, và lưu link đã upload vào lịch sử (`Recent Captures` / `History`) kèm nút **🔗 Link** & **🌐 Open**.
- **Nhúng tĩnh hoàn toàn thư viện C/C++ runtime (`libstdc++`, `libgcc`, `winpthread`, MSVC CRT `+crt-static`)** vào file `.exe`, khắc phục lỗi thiếu `libstdc++-6.dll` trên Windows 10/11 sạch.

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
