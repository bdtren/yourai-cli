cask "yourai" do
  arch arm: "aarch64", intel: "x86_64"

  version "__VERSION__"
  sha256 arm:   "__SHA256_ARM__",
         intel: "__SHA256_INTEL__"

  url "https://yourai.bdtren.com/cli/releases/v#{version}/yourai-v#{version}-#{arch}-apple-darwin.tar.gz"
  name "YourAI"
  desc "Build and run your own local AI"
  homepage "https://yourai.bdtren.com"

  livecheck do
    url "https://yourai.bdtren.com/cli/latest.txt"
    regex(/v?(\d+(?:\.\d+)+)/i)
  end

  depends_on macos: ">= :big_sur"

  binary "yourai"

  zap trash: "~/.yourai"
end
