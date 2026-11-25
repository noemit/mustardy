class Mustardy < Formula
  desc "Fast offline CLI that trims silences from videos and audio using FFmpeg"
  homepage "https://github.com/noemit/mustardy"
  version "0.2.0"
  license "MIT"

  depends_on "ffmpeg"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/noemit/mustardy/releases/download/v#{version}/mustardy-v#{version}-aarch64-apple-darwin.tar.gz"
      # sha256 "UPDATE_ON_RELEASE"
    else
      url "https://github.com/noemit/mustardy/releases/download/v#{version}/mustardy-v#{version}-x86_64-apple-darwin.tar.gz"
      # sha256 "UPDATE_ON_RELEASE"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/noemit/mustardy/releases/download/v#{version}/mustardy-v#{version}-aarch64-unknown-linux-gnu.tar.gz"
      # sha256 "UPDATE_ON_RELEASE"
    else
      url "https://github.com/noemit/mustardy/releases/download/v#{version}/mustardy-v#{version}-x86_64-unknown-linux-gnu.tar.gz"
      # sha256 "UPDATE_ON_RELEASE"
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
