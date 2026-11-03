if ($env:RUN_DEVICE -ceq 'true') {
  'Hosted source-bound acceptance preparation completed. The explicitly selected owner workload still requires its own bounded acceptance and independent retained-data review.' >> $env:GITHUB_STEP_SUMMARY
} else {
  'Hosted source-bound acceptance preparation completed. run_device=false schedules no owner-device job and closes no model or promotion gate. Review this exact-source package before explicitly selecting a future bounded device run; that run prepares and binds its own same-run package.' >> $env:GITHUB_STEP_SUMMARY
}

