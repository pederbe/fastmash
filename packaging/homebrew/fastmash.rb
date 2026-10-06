class Fastmash < Formula
  desc "Fast command-line statistics and table transformations"
  homepage "https://fastmash.io"
  url "https://github.com/pederbe/fastmash/archive/refs/tags/v0.1.0.tar.gz"
  sha256 "aa6b74c7e760622e615b44c5dbeb623aad50e4e6dc34a3808a3f3b1777964b60"
  license any_of: ["MIT", "Apache-2.0"]

  depends_on "python@3.14" => :build
  depends_on "rust" => :build
  depends_on arch: :x86_64
  depends_on :linux

  def install
    system "cargo", "install", *std_cargo_args(path: "crates/cli")
    notices = Utils.safe_popen_read("python3", "scripts/third_party_licenses.py")
    (buildpath/"THIRD-PARTY-LICENSES.md").write notices
    doc.install "README.md", "LICENSE-MIT", "LICENSE-APACHE", "THIRD-PARTY-LICENSES.md"
  end

  test do
    assert_path_exists bin/"fastmash-sort-supervisor"
    %w[LICENSE-MIT LICENSE-APACHE THIRD-PARTY-LICENSES.md].each do |document|
      assert_predicate doc/document, :size?
    end
    assert_equal "fastmash #{version}\n", shell_output("#{bin}/fastmash --version")
    assert_equal "6\t2\n", pipe_output("#{bin}/fastmash sum 1 mean 1", "1\n2\n3\n", 0)
    assert_equal "1.8171205928321\n", pipe_output("#{bin}/fastmash geomean 1", "1\n2\n3\n", 0)

    # A paired operation and route trace expose an unusable Sort supervisor.
    (testpath/"input").write "b\t2\t3\na\t1\t2\na\t3\t6\n"
    assert_equal "a\t2\nb\t0\n",
                 shell_output("LC_ALL=C FASTMASH_GROUPING=sort FASTMASH_SORT_TRACE=1 " \
                              "#{bin}/fastmash -sg1 pcov 2:3 <input 2>trace")
    assert_equal "sort route: system sort\n", (testpath/"trace").read
  end
end
