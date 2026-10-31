class Yourai < Formula
  desc "Build and run your own local AI"
  homepage "https://yourai.bdtren.com"
  license "MIT"

  livecheck do
    skip "Platform-specific URLs; check https://yourai.bdtren.com/cli/latest.txt"
  end

  on_macos do
    on_arm do
      url "https://yourai.bdtren.com/cli/releases/v__VERSION__/yourai-v__VERSION__-aarch64-apple-darwin.tar.gz"
      sha256 "__SHA_MAC_ARM__"
    end
    on_intel do
      url "https://yourai.bdtren.com/cli/releases/v__VERSION__/yourai-v__VERSION__-x86_64-apple-darwin.tar.gz"
      sha256 "__SHA_MAC_INTEL__"
    end
  end

  on_linux do
    on_arm do
      url "https://yourai.bdtren.com/cli/releases/v__VERSION__/yourai-v__VERSION__-aarch64-unknown-linux-musl.tar.gz"
      sha256 "__SHA_LINUX_ARM__"
    end
    on_intel do
      url "https://yourai.bdtren.com/cli/releases/v__VERSION__/yourai-v__VERSION__-x86_64-unknown-linux-musl.tar.gz"
      sha256 "__SHA_LINUX_INTEL__"
    end
  end

  def install
    bin.install "yourai"
  end

  test do
    assert_match "__VERSION__", shell_output("#{bin}/yourai --version")
  end
end
