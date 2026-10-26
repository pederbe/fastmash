class Fastmash < Formula
  desc "Fast command-line statistics and table transformations"
  homepage "https://fastmash.io"
  url "https://github.com/pederbe/fastmash/archive/refs/tags/v0.3.0.tar.gz"
  sha256 "9b4f2ffc01fabe03aacef9742c10dcedb02281d07b2fc5137dc7b78306b09258"
  license any_of: ["MIT", "Apache-2.0"]

  depends_on "rust" => :build

  on_macos do
    depends_on arch: :arm64
    depends_on macos: :sequoia
  end

  on_linux do
    depends_on arch: :x86_64
  end

  def install
    if OS.mac?
      # The Sort supervisor is Linux-only; macOS sorts with the built-in sorter.
      ENV["MACOSX_DEPLOYMENT_TARGET"] = "15.0"
      system "cargo", "install", "--bin", "fastmash", *std_cargo_args(path: "crates/cli")
      target = "aarch64-apple-darwin"
    else
      system "cargo", "install", *std_cargo_args(path: "crates/cli")
      target = "x86_64-unknown-linux-gnu"
    end
    notices = Utils.safe_popen_read("cargo", "run", "--locked", "--release", "-p", "fastmash-license-notices",
                                    "--", "--target", target)
    (buildpath/"THIRD-PARTY-LICENSES.md").write notices
    doc.install "README.md", "LICENSE-MIT", "LICENSE-APACHE", "THIRD-PARTY-LICENSES.md"
    cd "packaging/homebrew" do
      system "rustc", "--edition=2024", "--target", target, "-C", "linker=#{ENV.cc}",
             "-C", "opt-level=2", "-C", "strip=symbols", "run_test.rs", "-o", "fastmash-test-homebrew"
      libexec.install "fastmash-test-homebrew"
    end
  end

  test do
    system libexec/"fastmash-test-homebrew", bin, doc, version.to_s
  end
end
