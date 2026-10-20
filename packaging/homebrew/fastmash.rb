class Fastmash < Formula
  desc "Fast command-line statistics and table transformations"
  homepage "https://fastmash.io"
  url "https://github.com/pederbe/fastmash/archive/refs/tags/v0.2.0.tar.gz"
  sha256 "e331d2d9499dc502f16e253ab4c408ceed4cca251420c53718ea8f70595e0302"
  license any_of: ["MIT", "Apache-2.0"]

  depends_on "python@3.14" => :build
  depends_on "rust" => :build

  on_macos do
    depends_on arch: :arm64
    depends_on macos: :sequoia
  end

  on_linux do
    depends_on arch: :x86_64
  end

  resource "package-test" do
    url "https://raw.githubusercontent.com/pederbe/fastmash/e8ec726952adc36070dc381fbb46394c9541dee7/packaging/homebrew/run_test.rs"
    sha256 "74982a7204196bccb4c684f1961d55e99d61c85c53899efd025fdfd7100059d9"
  end

  def install
    if OS.mac?
      # The Sort supervisor is Linux-only; macOS sorts with the built-in sorter.
      system "cargo", "install", "--bin", "fastmash", *std_cargo_args(path: "crates/cli")
      target = "aarch64-apple-darwin"
    else
      system "cargo", "install", *std_cargo_args(path: "crates/cli")
      target = "x86_64-unknown-linux-gnu"
    end
    notices = Utils.safe_popen_read("python3", "scripts/third_party_licenses.py", "--target", target)
    (buildpath/"THIRD-PARTY-LICENSES.md").write notices
    doc.install "README.md", "LICENSE-MIT", "LICENSE-APACHE", "THIRD-PARTY-LICENSES.md"
    resource("package-test").stage do
      system "rustc", "--edition=2024", "--target", target, "-C", "linker=#{ENV.cc}",
             "-C", "opt-level=2", "-C", "strip=symbols", "run_test.rs", "-o", "fastmash-test-homebrew"
      libexec.install "fastmash-test-homebrew"
    end
  end

  test do
    system libexec/"fastmash-test-homebrew", bin, doc, version.to_s
  end
end
