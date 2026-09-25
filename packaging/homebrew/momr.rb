# Homebrew formula for MOM Recorder. Lives in a tap repository
# (<github-user>/homebrew-tap as `Formula/momr.rb`); staged here so the
# release commit can copy it over. Version and sha256 are filled in when the
# first release is tagged (plan 11); until then `brew install
# --build-from-source` against this file only works from a tarball.
class Momr < Formula
  desc "MOM Recorder: record a meeting in two tracks and transcribe it on this computer"
  homepage "https://github.com/riobahtiar/mo-meeting-recorder"
  url "https://github.com/riobahtiar/mo-meeting-recorder/archive/refs/tags/v2.0.0.tar.gz"
  sha256 "FILL-IN-AT-RELEASE"
  license "MIT"

  depends_on "cmake" => :build
  depends_on "pkgconf" => :build
  depends_on "rust" => :build
  depends_on xcode: :build # swift build for the helpers
  depends_on "adwaita-icon-theme"
  depends_on "dylibbundler" => :build
  depends_on "ffmpeg"
  depends_on "gtk4"
  depends_on "libadwaita"
  depends_on macos: :sonoma

  def install
    system "cargo", "install", "--features", "metal", *std_cargo_args
    system "swift", "build", "-c", "release", "--package-path", "helpers/momr-audio"
    bin.install "helpers/momr-audio/.build/release/momr-audio"
    bin.install "helpers/momr-audio/.build/release/momr-menubar"
  end

  test do
    assert_match "Usage", shell_output("#{bin}/momr --help")
    system bin/"momr-audio", "list"
  end
end
