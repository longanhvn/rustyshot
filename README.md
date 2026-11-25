<p align="center">
  <img src="assets/icon.png" alt="RustyShot Icon" width="84" />
</p>

# RustyShot

**RustyShot** là phần mềm chụp ảnh, quay màn hình, chú thích và upload nhanh đa nền tảng (**Windows**, **Ubuntu/Linux**, **macOS**) viết bằng **Rust**, gọn nhẹ, **100% portable** (chỉ 1 file thực thi duy nhất, không cần cài đặt hay phụ thuộc thư viện ngoài).

> 📦 Tải bản phát hành mới nhất cho Windows (`.exe`), Ubuntu/Debian (`.deb` / portable) và macOS (`.dmg` Apple Silicon & Intel) tại: **[GitHub Releases](https://github.com/longanhvn/rustyshot/releases)**

## Giao diện ứng dụng

![RustyShot Dashboard](docs/screenshots/rustyshot-dashboard.png)

## Tính năng nổi bật

- **Chụp ảnh màn hình đa chế độ:**
  - **Chụp vùng chọn** (`Ctrl+Shift+S`): kính lúp phóng đại 11×11, thước căn dòng (crosshair), thanh công cụ nổi ở cạnh trên sau khi chọn vùng, giữ `Shift` khóa tỉ lệ vuông, phím mũi tên tinh chỉnh `1px` / `10px`, 8 điểm kéo kích thước, tự động bắt khung cửa sổ khi rê chuột.
  - **Chụp toàn màn hình** (`Ctrl+Shift+F`), **chụp theo cửa sổ**, hẹn giờ chụp (`0s / 3s / 5s`).
  - Dán ảnh trực tiếp từ Clipboard (`Ctrl+V`) hoặc kéo thả file ảnh vào ứng dụng.
- **Quay màn hình (tích hợp sẵn MP4 H.264 & GIF — không cần FFmpeg ngoài):**
  - **Record Full** (`Ctrl+Shift+R`): quay toàn bộ màn hình chính.
  - **Record Region**: kéo chọn vùng bất kỳ trên màn hình để quay, hiển thị **4 đường viền đỏ mảnh** bao quanh vùng đang quay (bấm chuột xuyên qua hoàn toàn, không che nội dung).
  - Thanh điều khiển nổi nhỏ gọn kèm nút **⏹ Stop** xử lý bất đồng bộ (không đơ giao diện khi đóng gói video dài).
- **Trình chỉnh sửa & chú thích nhanh:**
  - 9 công cụ: Mũi tên, Khung chữ nhật, Hình tròn/Elip, Bút vẽ tay, Bút đánh dấu (Highlight), Đánh số thứ tự (Step badge), Văn bản, Làm mờ/Che thông tin (Pixelate), Cắt ảnh (Crop).
  - Hỗ trợ Undo/Redo, Zoom (`-` / `100%` / `+` / `Fit` / `Ctrl+Cuộn`), Xoay 90°, Lật ngang/dọc.
- **Ghim ảnh nổi (Pinned Shot):**
  - Ghim ảnh chụp lên trên các cửa sổ khác, hỗ trợ phóng to/thu nhỏ và chỉnh độ mờ (Opacity).
- **Upload tự động & bảo mật (SFTP / FTPS / FTP):**
  - Hỗ trợ **SFTP / SSH (Port 22)**, **FTPS (Explicit TLS)** và **FTP**.
  - **Mã hóa mật khẩu AES-256-GCM** trên đĩa (`data/RustyShot.json`), không lưu mật khẩu dạng văn bản rõ.
  - Tùy chọn **Auto-upload**, **Open after upload** (tự mở trình duyệt sau khi upload), tự động copy Public URL vào Clipboard, lưu link vào lịch sử (`🔗 Link`, `🌐 Open`) và thanh trạng thái upload cố định ở viền dưới.
- **Phím tắt toàn cục linh hoạt (Click-to-Bind Hotkeys):**
  - Nhấp trực tiếp vào ô phím tắt trong **Settings → Global Hotkeys** rồi gõ phím để gán và lưu ngay lập tức.
  - Hỗ trợ gán tổ hợp phím (`Ctrl+Shift+S`...) hoặc **1 phím đơn `PrtSc` (PrintScreen)**.
- **Hiệu năng cao & Bất đồng bộ:**
  - Nén PNG tốc độ cao (`Fast` + `Sub`), lưu file ảnh, đóng gói video và tải ảnh thu nhỏ (Thumbnail) chạy hoàn toàn trên luồng nền, giao diện luôn mượt mà.
  - Định dạng mặc định `.png` lossless; toàn bộ tên file đầu ra chuẩn chữ thường (`rustyshot_YYYY-MM-DD_HH-MM-SS.png`, `rustyshot_rec_YYYY-MM-DD_HH-MM-SS.mp4`).
- **100% Portable & Single Instance:**
  - Tự động lưu cấu hình và media ngay cạnh file chạy: `./data/RustyShot.json`, `./data/image/`, `./data/video/`.
  - Chỉ chạy 1 tiến trình duy nhất; thu gọn xuống khay hệ thống (System Tray) và khôi phục nhanh khi click icon Tray hoặc mở lại ứng dụng.
  - Windows `.exe` nhúng tĩnh hoàn toàn C/C++ runtime, chạy trực tiếp trên Windows 10/11 sạch không cần cài thêm DLL.

## Build từ mã nguồn

```bash
# Kiểm tra & chạy trên Linux
cargo clippy -- -D warnings
cargo test
cargo run --release

# Cross-compile Windows x64 Portable trên Linux/WSL (tự động nhúng tĩnh C/C++ runtime)
cargo build --release --target x86_64-pc-windows-gnu
x86_64-w64-mingw32-strip target/x86_64-pc-windows-gnu/release/rustyshot.exe
```

## License

MIT
