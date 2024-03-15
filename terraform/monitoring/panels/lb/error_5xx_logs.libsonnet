local grafana   = import '../../grafonnet-lib/grafana.libsonnet';
local defaults  = import '../../grafonnet-lib/defaults.libsonnet';

local cloudwatch_target = import '../../grafonnet-lib/targets/cloudwatch.libsonnet';

local panels    = grafana.panels;
local targets   = grafana.targets;

{
  new(ds, vars)::
    panels.timeseries(
      title       = 'HTTP 5xx Errors',
      datasource  = ds.cloudwatch,
    )
    .configure({
      fieldConfig: {},
      options: {
        showHeader: false,
      },
    })

    .addTarget(targets.cloudwatch(
      datasource  = ds.cloudwatch,
      queryMode   = cloudwatch_target.queryModes.Logs,
      logGroups   = [{
        arn: vars.log_group_arn,
        name: vars.log_group_name,
        accountId: vars.aws_account_id,
      }],
      expression = 'fields @timestamp, @message, @logStream, @log\n| filter @message like /HTTP server error/\n| parse @message /^(?<LogTimestamp>[^\\s]+)/\n| display @message\n| sort LogTimestamp desc',
      refId       = '5xx_Errors',
    ))
}
