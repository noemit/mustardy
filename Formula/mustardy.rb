class Mustardy < Formula
  desc "Fast offline Rust + FFmpeg CLI that trims silences from video & audio"
  homepage "https://mustardy.com"
  version "0.2.0"
  license "MIT"

  depends_on "ffmpeg"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/noemit/mustardy/releases/download/v#{version}/mustardy-v#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "3a11704d8ba8b3b3bcc9dedc0d28b355d27ffe3939bb06cedf6a64177efe568f"
    else
      url "https://github.com/noemit/mustardy/releases/download/v#{version}/mustardy-v#{version}-x86_64-apple-darwin.tar.gz"
      sha256 "759c9b2c2a8ab121630fa1869b37fb7ce81fb738517ecfa838366c9df9908908"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/noemit/mustardy/releases/download/v#{version}/mustardy-v#{version}-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "193b6a43bf09e776f59ed4f04264749c32dcd2d94613158b276037c2405e52e3"
    else
      url "https://github.com/noemit/mustardy/releases/download/v#{version}/mustardy-v#{version}-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "0a4d428c4028223a7533ca22f2c66db46b0e21deaac4ab8ed2d4f6fb182b7806"
    end
  end

  head "https://github.com/noemit/mustardy.git", branch: "main"

  def install
    if build.head?
      system "cargo", "install", *std_cargo_args
    else
      bin.install "mustardy"
    end
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/mustardy --version")
  end
end
