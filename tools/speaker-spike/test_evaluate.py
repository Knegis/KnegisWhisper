import json
import tempfile
import unittest
from pathlib import Path
from evaluate import load, score, digest


class EvaluationTests(unittest.TestCase):
    def test_unknown_and_ambiguous_speakers_are_rejected(self):
        a=[1.,0.]+[0.]*510
        b=[0.,1.]+[0.]*510
        rows=[dict(split="enroll",person="a",vector=a,seconds=10),
              dict(split="test",person="a",vector=a,seconds=10,session="test",speaker_id="s0"),
              dict(split="test",person="b",vector=b,seconds=10,session="test",speaker_id="s1")]
        policy=dict(threshold=.9,margin=.1,min_seconds=3)
        result=score(rows,policy,"test")
        self.assertEqual(result["correct"],1)
        self.assertEqual(result["unknown_accepted"],0)
        self.assertFalse(result["passed"])
        rows.append(dict(split="enroll",person="c",vector=a,seconds=10))
        self.assertEqual(score(rows,policy,"test")["correct"],0)

    def test_calibration_never_reads_test_samples(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp)
            (root/"a.json").write_text(json.dumps(dict(model_id="test",format="test",samples=[dict(speaker_id="s",seconds=5,embedding=[1.]+[0.]*511)])))
            rows=[dict(samples_file="a.json",speaker_id="s",person="a",session="one",day="one",split="enroll"),
                  dict(samples_file="does-not-exist.json",speaker_id="s",person="a",session="two",day="two",split="test")]
            rows[0]["samples_sha256"]=digest(root/"a.json")
            rows[1]["samples_sha256"]="0"*64
            manifest=root/"manifest.json";manifest.write_text(json.dumps(rows))
            self.assertEqual(len(load(manifest,{"enroll","calibrate"})[0]),1)
            rows[1]["session"]="one";manifest.write_text(json.dumps(rows))
            with self.assertRaises(ValueError): load(manifest,{"enroll"})


if __name__ == "__main__": unittest.main()
