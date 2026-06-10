class Caffeinate < Formula
  desc "Cross-platform caffeinate command for Linux and Windows"
  homepage "https://github.com/i-rocky/caffeinate"
  version "0.1.1"
  license "MIT"

  # macOS ships caffeinate natively; this formula is Linux-only on purpose.
  depends_on :linux

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/i-rocky/caffeinate/releases/download/v0.1.1/caffeinate-linux-aarch64-v0.1.1.tar.gz"
      sha256 "REPLACE_LINUX_ARM64_SHA256"
    else
      url "https://github.com/i-rocky/caffeinate/releases/download/v0.1.1/caffeinate-linux-x86_64-v0.1.1.tar.gz"
      sha256 "REPLACE_LINUX_X86_64_SHA256"
    end
  end

  def install
    bin.install "caffeinate"
  end

  test do
    assert_predicate bin/"caffeinate", :exist?
  end
end
