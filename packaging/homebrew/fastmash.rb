class Fastmash < Formula
  desc "Fast command-line statistics and table transformations"
  homepage "https://fastmash.io"
  url "https://github.com/pederbe/fastmash/releases/download/v0.4.0/fastmash-v0.4.0-aarch64-apple-darwin.tar.gz"
  sha256 "d72fc394de0d4ee043f0b5eda689a002e0aabed9a9c1a09529e3333e89402128"
  license any_of: ["MIT", "Apache-2.0"]
  revision 1

  on_macos do
    depends_on arch: :arm64
    depends_on macos: :sequoia
  end

  on_linux do
    depends_on arch: :x86_64
    on_intel do
      url "https://github.com/pederbe/fastmash/releases/download/v0.4.0/fastmash-v0.4.0-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "1abb9eb310253aa60d147da51aaf9878dcad4ab3cf08795fa3444538b7e66b27"
    end
  end

  def install
    bin.install "fastmash"
    doc.install "README.md", "CHANGELOG.md", "LICENSE-MIT", "LICENSE-APACHE", "THIRD-PARTY-LICENSES.md"
  end

  test do
    system bin/"fastmash", "--version"
  end
end
