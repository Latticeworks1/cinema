"""Cross-language boundary fixtures and audio interval coverage."""
import importlib.util
from pathlib import Path
import unittest

path = Path(__file__).resolve().parents[1] / 'python/output_contract.py'
spec = importlib.util.spec_from_file_location('output_contract', path)
contract = importlib.util.module_from_spec(spec)
spec.loader.exec_module(contract)


class OutputContractTests(unittest.TestCase):
    def test_half_frame_matches_rust(self):
        self.assertEqual(contract.frame_count(0.15), 5)
        self.assertEqual(contract.frame_count(31.0), 930)

    def test_fractional_sample_hops_cover_exact_seconds(self):
        bounds = [contract.sample_bounds(i, 22051) for i in range(30)]
        self.assertEqual(bounds[0][0], 0)
        self.assertEqual(bounds[-1][1], 22051)
        self.assertTrue(all(a[1] == b[0] for a, b in zip(bounds, bounds[1:])))

    def test_invalid_duration(self):
        for value in [0, -1, float('nan'), float('inf')]:
            with self.assertRaises(ValueError):
                contract.frame_count(value)
