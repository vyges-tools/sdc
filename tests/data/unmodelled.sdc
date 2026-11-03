create_clock -name clk -period 10 [get_ports clk]
set_assigned_delay -net -from [get_ports din] -to [get_ports dout] 0.1
