CREATE INDEX canary_workbench_time ON canaries(source,julianday(json_extract(payload_json,'$.send_wall_utc')),id);
CREATE INDEX canary_resolution_tail ON resolution_history(canary_id,id);
CREATE INDEX forecast_grade_tail ON forecast_grades(forecast_hash,id);
