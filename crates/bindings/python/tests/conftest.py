"""Shared test fixtures for quillmark tests.

These fixtures prefer using the canonical repository fixtures located in
`crates/fixtures/resources`. If those resources cannot be found the
original simple fallbacks are used so tests remain robust in odd layouts.
"""

from pathlib import Path
import pytest

from quillmark import Quillmark

WORKSPACE_ROOT = Path(__file__).resolve().parents[4]
RESOURCES_PATH = WORKSPACE_ROOT / "crates" / "fixtures" / "resources"
QUILLS_PATH = RESOURCES_PATH / "quills"


def _latest_version(quill_dir: Path) -> Path:
    """Return the latest versioned subdirectory of a quill, or the dir itself."""
    if (quill_dir / "Quill.yaml").exists():
        return quill_dir
    versions = sorted(
        (p.name for p in quill_dir.iterdir() if p.is_dir()),
        key=lambda v: [int(x) for x in v.split(".") if x.isdigit()],
    )
    if versions:
        return quill_dir / versions[-1]
    return quill_dir


@pytest.fixture
def taro_quill_dir():
    """Return the path to the taro fixture quill (latest version).

    Tests must not mutate it in place.
    """
    fixture_path = _latest_version(QUILLS_PATH / "taro")

    assert fixture_path.exists(), f"Preferred fixture not found: {fixture_path}"

    return fixture_path


@pytest.fixture
def engine():
    """A fresh `Quillmark` engine — the render / capability dispatcher.

    A `Quill` is engine-free, validated data (`Quill.from_path`); rendering
    (`render`) and the format probe (`supported_formats`) are asked of the
    engine, against a quill.
    """
    return Quillmark()


TARO_MARKDOWN = '''~~~card-yaml
$quill: taro@0.1
$kind: main
author: Nibs
ice_cream: Taro
title: "My Favorite Ice Cream Flavor"
~~~

I love Taro ice cream for its subtly sweet, nutty flavor and creamy, earthy undertones.

~~~card-yaml
$kind: quotes
author: Albert Einstein
~~~
Without taro ice cream, life would be a mistake.
'''


@pytest.fixture
def taro_md():
    """Return a sample taro markdown document.

    The test owns its input — it does not depend on a bundled fixture file.
    """
    return TARO_MARKDOWN
